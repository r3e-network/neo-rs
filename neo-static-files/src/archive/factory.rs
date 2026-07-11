//! Factory open path, format initialization, and bounded suffix recovery.

use std::fs::OpenOptions;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lru::LruCache;
use parking_lot::Mutex;
use xxhash_rust::xxh3::Xxh3;

use super::index::{
    ArchiveIndex, FrameLocation, ScanMode, ScannedFrame, scan_archive, validate_published_tail,
};
use super::io::{read_exact_at, sync_parent_directory, write_all_at};
use super::lease::WriterLease;
use super::provider::{ArchiveInner, StaticFileArchive};
use super::{StaticFileConfig, StaticFileProviderFactory};
use crate::format::{FILE_HEADER_LEN, file_header, validate_file_header};
use crate::{StaticFileError, StaticFileResult};

const INDEX_PUBLICATION_BATCH_FRAMES: usize = 1_024;

/// Work performed while opening a static archive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticFileOpenStats {
    /// Archive frames decoded because they followed the durable index checkpoint.
    pub frames_scanned: u64,
    /// Compressed payloads decoded while validating that suffix.
    pub payloads_decoded: u64,
    /// Row-location versions replayed into MDBX.
    pub rows_replayed: u64,
    /// Whether an incompatible or ahead index was discarded and rebuilt.
    pub index_rebuilt: bool,
    /// Whether incomplete unpublished archive bytes were removed.
    pub archive_tail_truncated: bool,
}

/// Factory for the versioned append-only file provider.
#[derive(Clone, Copy, Debug, Default)]
pub struct StaticFileArchiveFactory {
    config: StaticFileConfig,
}

impl StaticFileArchiveFactory {
    /// Creates a factory with explicit limits and compression policy.
    #[must_use]
    pub const fn new(config: StaticFileConfig) -> Self {
        Self { config }
    }

    /// Returns the configured archive policy.
    #[must_use]
    pub const fn config(self) -> StaticFileConfig {
        self.config
    }

    /// Opens the archive and reports whether suffix recovery work was required.
    pub fn open_with_stats(
        &self,
        path: &Path,
    ) -> StaticFileResult<(StaticFileArchive, StaticFileOpenStats)> {
        self.config.validate()?;
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|source| StaticFileError::io("create directory", parent, source))?;
        }
        let existed = path.exists();
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|source| StaticFileError::io("open", path, source))?;
        WriterLease::acquire(&file, path)?;
        let mut file_len = file
            .metadata()
            .map_err(|source| StaticFileError::io("read metadata", path, source))?
            .len();

        let archive_id =
            if file_len < u64::try_from(FILE_HEADER_LEN).expect("header length fits u64") {
                let archive_id = create_archive_id(path);
                file.set_len(0)
                    .map_err(|source| StaticFileError::io("reset partial header", path, source))?;
                write_all_at(&file, 0, &file_header(archive_id))
                    .map_err(|source| StaticFileError::io("write header", path, source))?;
                file.sync_all()
                    .map_err(|source| StaticFileError::io("sync header", path, source))?;
                if !existed {
                    sync_parent_directory(path)?;
                }
                file_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
                archive_id
            } else {
                let mut header = [0u8; FILE_HEADER_LEN];
                read_exact_at(&file, 0, &mut header)
                    .map_err(|source| StaticFileError::io("read header", path, source))?;
                validate_file_header(&header, 0)?
            };

        let index = ArchiveIndex::open(path)?;
        let prior_state = index.state();
        let state_is_usable = prior_state.is_some_and(|state| {
            state.archive_id == archive_id
                && state.indexed_file_len <= file_len
                && index_tail_matches(&index, &file, path, self.config, state)
        });
        let mut stats = StaticFileOpenStats::default();
        let state = if state_is_usable {
            prior_state.expect("usable state is present")
        } else {
            stats.index_rebuilt = prior_state.is_some()
                || file_len > u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
            let empty_archive =
                file_len == u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
            index.reset(archive_id, empty_archive)?
        };
        let scan_mode = if state.tail_recovery_safe {
            ScanMode::RecoverTail
        } else {
            ScanMode::Strict
        };

        let mut batch = Vec::<ScannedFrame>::with_capacity(INDEX_PUBLICATION_BATCH_FRAMES);
        let outcome = scan_archive(
            &file,
            path,
            self.config,
            file_len,
            state.indexed_file_len,
            state.next_height()?,
            scan_mode,
            |frame| {
                batch.push(frame);
                if batch.len() == INDEX_PUBLICATION_BATCH_FRAMES {
                    index.publish_frames(&batch)?;
                    batch.clear();
                }
                Ok(())
            },
        )?;
        if !batch.is_empty() {
            index.publish_frames(&batch)?;
        }
        stats.frames_scanned = outcome.frames_scanned;
        stats.payloads_decoded = outcome.payloads_decoded;
        stats.rows_replayed = outcome.rows_scanned;

        if outcome.valid_file_len != file_len {
            file.set_len(outcome.valid_file_len)
                .map_err(|source| StaticFileError::io("truncate torn tail", path, source))?;
            file.sync_all()
                .map_err(|source| StaticFileError::io("sync recovered tail", path, source))?;
            stats.archive_tail_truncated = true;
            file_len = outcome.valid_file_len;
        }
        index.mark_tail_recovery_safe()?;
        debug_assert_eq!(
            index.state().map(|state| state.indexed_file_len),
            Some(file_len)
        );

        Ok((
            StaticFileArchive {
                inner: Arc::new(ArchiveInner {
                    file,
                    path: path.to_path_buf(),
                    config: self.config,
                    write_lock: Mutex::new(()),
                    index,
                    cache: Mutex::new(LruCache::new(
                        NonZeroUsize::new(self.config.cache_capacity)
                            .expect("config validation rejects a zero cache"),
                    )),
                    healthy: AtomicBool::new(true),
                }),
            },
            stats,
        ))
    }
}

impl StaticFileProviderFactory for StaticFileArchiveFactory {
    type Provider = StaticFileArchive;

    fn open(&self, path: &Path) -> StaticFileResult<Self::Provider> {
        self.open_with_stats(path).map(|(archive, _)| archive)
    }
}

fn index_tail_matches(
    index: &ArchiveIndex,
    file: &std::fs::File,
    path: &Path,
    config: StaticFileConfig,
    state: super::index::IndexState,
) -> bool {
    if validate_published_tail(file, path, config, state).is_err() {
        return false;
    }
    match state.tip {
        Some(height) => index.frame(height).is_ok_and(|location| {
            location
                == Some(FrameLocation {
                    height,
                    start: state.last_frame_start,
                    end: state.indexed_file_len,
                })
        }),
        None => true,
    }
}

fn create_archive_id(path: &Path) -> u64 {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = Xxh3::new();
    hasher.update(path.as_os_str().as_encoded_bytes());
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&elapsed.to_le_bytes());
    hasher.update(&NEXT_ID.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hasher.digest().max(1)
}

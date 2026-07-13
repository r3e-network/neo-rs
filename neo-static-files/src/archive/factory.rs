//! Factory open path, format initialization, and bounded suffix recovery.

use std::fs::OpenOptions;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lru::LruCache;
use parking_lot::{Mutex, RwLock};
use xxhash_rust::xxh3::Xxh3;

use super::index::ArchiveIndex;
use super::io::{read_exact_at, sync_parent_directory, write_all_at};
use super::lease::WriterLease;
use super::provider::{ArchiveInner, StaticFileArchive};
use super::recovery::{indexed_layout_matches, recover_segments};
use super::segments::ArchiveSegments;
use super::{StaticFileConfig, StaticFileProviderFactory};
use crate::format::{FILE_HEADER_LEN, file_header, validate_file_header};
use crate::{StaticFileError, StaticFileResult};

/// Work performed while opening a static archive.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StaticFileOpenStats {
    /// Height-addressed archive segment files retained after recovery.
    pub segments_retained: u32,
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

        let mut segments = ArchiveSegments::discover(path, file, archive_id)?;
        let mut stats = StaticFileOpenStats {
            segments_retained: u32::try_from(segments.count()).unwrap_or(u32::MAX),
            ..StaticFileOpenStats::default()
        };
        let index = ArchiveIndex::open(path)?;
        let prior_state = index.state();
        let state_is_usable = prior_state.is_some_and(|state| {
            state.archive_id == archive_id
                && indexed_layout_matches(&index, &segments, self.config, state)
        });
        let state = if state_is_usable {
            prior_state.expect("usable state is present")
        } else {
            let snapshots = segments.snapshots();
            stats.index_rebuilt = prior_state.is_some()
                || snapshots.len() > 1
                || file_len > u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
            let empty_archive = snapshots.len() == 1
                && file_len == u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
            index.reset(archive_id, empty_archive)?
        };
        recover_segments(
            &index,
            &mut segments,
            self.config,
            state,
            !state_is_usable,
            &mut stats,
        )?;
        index.mark_tail_recovery_safe()?;
        let indexed_state = index
            .state()
            .ok_or_else(|| StaticFileError::invalid_index("archive index lost its state"))?;
        let active_segment = segments.exact(indexed_state.active_segment_start)?;
        debug_assert_eq!(active_segment.len()?, indexed_state.indexed_file_len);

        Ok((
            StaticFileArchive {
                inner: Arc::new(ArchiveInner {
                    path: path.to_path_buf(),
                    config: self.config,
                    segments: RwLock::new(segments),
                    write_lock: Mutex::new(()),
                    pending: Mutex::new(None),
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

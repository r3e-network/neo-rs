//! Cloneable archive provider and staged publication/read/truncate operations.

use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lru::LruCache;
use parking_lot::Mutex;

use super::StaticFileProvider;
use super::config::StaticFileConfig;
use super::index::{
    ArchiveIndex, PositionedEncodedFrame, RowLocation, ScanMode, ScannedFrame, read_frame_index,
    scan_archive,
};
use super::io::{read_exact_at, write_all_at};

use crate::format::{FILE_HEADER_LEN, encode_frame};
use crate::{StaticFileError, StaticFileResult, StaticRecord};

/// Cloneable append/read handle over one static archive file.
#[derive(Clone)]
pub struct StaticFileArchive {
    pub(super) inner: Arc<ArchiveInner>,
}

impl fmt::Debug for StaticFileArchive {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StaticFileArchive")
            .field("path", &self.inner.path)
            .field("tip", &self.tip())
            .field("healthy", &self.is_healthy())
            .finish()
    }
}

impl StaticFileArchive {
    /// Writes and syncs a contiguous record batch without publishing its index.
    ///
    /// The staged frames are invisible to readers until
    /// [`Self::publish_staged_append`] succeeds. This is the visibility half of
    /// the cold-first canonical commit protocol: durable cold bytes may exist
    /// before the hot transaction, but no provider can route a read to them.
    pub fn stage_append(&self, records: Vec<StaticRecord>) -> StaticFileResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        let _write_guard = self.inner.write_lock.lock();
        self.stage_append_locked(records)
    }

    fn stage_append_locked(&self, records: Vec<StaticRecord>) -> StaticFileResult<()> {
        if !self.is_healthy() {
            return Err(StaticFileError::Unhealthy);
        }
        if self.inner.pending.lock().is_some() {
            return Err(StaticFileError::invalid_index(
                "static archive already has an unpublished append",
            ));
        }

        validate_continuity(self.tip(), &records)?;
        let mut encoded = Vec::with_capacity(records.len());
        for record in records {
            encoded.push(encode_frame(record, self.inner.config)?);
        }

        let start = self
            .inner
            .index
            .state()
            .ok_or_else(|| StaticFileError::invalid_index("archive index is not initialized"))?
            .indexed_file_len;
        let mut offset = start;
        let mut frames = Vec::with_capacity(encoded.len());
        for frame in encoded {
            let end = offset
                .checked_add(frame.header.frame_len)
                .ok_or_else(|| StaticFileError::invalid(start, "archive offset overflow"))?;
            if let Err(source) = write_all_at(&self.inner.file, offset, &frame.bytes) {
                self.inner.healthy.store(false, Ordering::Release);
                return Err(StaticFileError::io(
                    "stage archive frame",
                    &self.inner.path,
                    source,
                ));
            }
            frames.push(PositionedEncodedFrame::new(offset, frame));
            offset = end;
        }
        if let Err(source) = self.inner.file.sync_data() {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(StaticFileError::io(
                "sync staged archive frames",
                &self.inner.path,
                source,
            ));
        }
        *self.inner.pending.lock() = Some(PendingAppend { frames });
        Ok(())
    }

    /// Publishes the index for a previously staged append.
    ///
    /// Returns `Ok(())` when no append is pending as well. If publication
    /// fails, the bytes remain an unpublished suffix for strict startup
    /// recovery; the handle is poisoned so the caller can stop the writer.
    pub fn publish_staged_append(&self) -> StaticFileResult<()> {
        let _write_guard = self.inner.write_lock.lock();
        self.publish_staged_append_locked()
    }

    fn publish_staged_append_locked(&self) -> StaticFileResult<()> {
        let Some(pending) = self.inner.pending.lock().take() else {
            return Ok(());
        };
        if let Err(error) = self.inner.index.publish_frames(&pending.frames) {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }

    /// Appends and durably flushes one finalized-height record.
    pub fn append(&self, record: StaticRecord) -> StaticFileResult<()> {
        self.append_batch(vec![record])
    }

    /// Appends a contiguous record batch and performs one durability sync.
    pub fn append_batch(&self, records: Vec<StaticRecord>) -> StaticFileResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        let _write_guard = self.inner.write_lock.lock();
        self.stage_append_locked(records)?;
        self.publish_staged_append_locked()
    }

    /// Truncates every frame above `height` and rolls back indexed row versions.
    ///
    /// Passing `None` resets the archive to its versioned file header. Passing
    /// a height at or above the current tip is a no-op.
    pub fn truncate_after(&self, height: Option<u32>) -> StaticFileResult<()> {
        let _write_guard = self.inner.write_lock.lock();
        if self.inner.pending.lock().is_some() {
            return Err(StaticFileError::invalid_index(
                "cannot truncate while an archive append is unpublished",
            ));
        }
        let state =
            self.inner.index.state().ok_or_else(|| {
                StaticFileError::invalid_index("archive index is not initialized")
            })?;
        if height.is_some_and(|target| state.tip.is_none_or(|tip| target >= tip)) {
            return Ok(());
        }
        let target_len = match height {
            None => u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
            Some(target) => {
                self.inner
                    .index
                    .frame(target)?
                    .ok_or_else(|| {
                        StaticFileError::invalid_index("truncate target frame is missing")
                    })?
                    .end
            }
        };
        if target_len == state.indexed_file_len {
            return Ok(());
        }

        let removed = self
            .inner
            .index
            .frames_after(height)?
            .into_iter()
            .map(|frame| {
                read_frame_index(&self.inner.file, &self.inner.path, self.inner.config, frame)
            })
            .collect::<StaticFileResult<Vec<_>>>()?;

        self.inner
            .file
            .set_len(target_len)
            .map_err(|source| StaticFileError::io("truncate", &self.inner.path, source))?;
        self.inner
            .file
            .sync_all()
            .map_err(|source| StaticFileError::io("sync truncation", &self.inner.path, source))?;
        if let Err(error) = self.inner.index.truncate_after(height, &removed) {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(error);
        }
        self.inner.cache.lock().clear();
        self.inner.healthy.store(true, Ordering::Release);
        Ok(())
    }

    /// Returns the archive path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Returns the path-adjacent MDBX index directory.
    #[must_use]
    pub fn index_path(&self) -> &Path {
        self.inner.index.path()
    }

    /// Strictly verifies every archive frame and every persistent index entry.
    ///
    /// Normal startup trusts the durable MDBX checkpoint and validates only an
    /// unpublished suffix. Operators can run this full scrub when they need an
    /// eager media-integrity check instead of on-demand frame verification.
    pub fn scrub(&self) -> StaticFileResult<()> {
        let _write_guard = self.inner.write_lock.lock();
        let state =
            self.inner.index.state().ok_or_else(|| {
                StaticFileError::invalid_index("archive index is not initialized")
            })?;
        let file_len = self
            .inner
            .file
            .metadata()
            .map_err(|source| StaticFileError::io("read scrub metadata", &self.inner.path, source))?
            .len();
        if file_len != state.indexed_file_len {
            return Err(StaticFileError::invalid_index(
                "archive and index lengths differ during scrub",
            ));
        }

        const VERIFY_BATCH_FRAMES: usize = 1_024;
        let mut batch = Vec::<ScannedFrame>::with_capacity(VERIFY_BATCH_FRAMES);
        let outcome = scan_archive(
            &self.inner.file,
            &self.inner.path,
            self.inner.config,
            file_len,
            u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
            0,
            ScanMode::Strict,
            |frame| {
                batch.push(frame);
                if batch.len() == VERIFY_BATCH_FRAMES {
                    self.inner.index.verify_frames(&batch)?;
                    batch.clear();
                }
                Ok(())
            },
        )?;
        if !batch.is_empty() {
            self.inner.index.verify_frames(&batch)?;
        }
        let expected_frames = state.tip.map_or(0, |tip| u64::from(tip) + 1);
        if outcome.valid_file_len != file_len
            || outcome.frames_scanned != expected_frames
            || self.inner.index.stored_frame_count()? != expected_frames
            || outcome.rows_scanned != state.row_versions
            || self.inner.index.stored_row_versions()? != state.row_versions
        {
            return Err(StaticFileError::invalid_index(
                "archive scrub counts disagree with the persistent index",
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn insert_test_frame_location(
        &self,
        height: u32,
        start: u64,
        end: u64,
    ) -> StaticFileResult<()> {
        self.inner
            .index
            .insert_test_frame(super::index::FrameLocation { height, start, end })
    }

    /// Returns whether no append durability failure has poisoned this handle.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.inner.healthy.load(Ordering::Acquire)
    }

    fn load_payload(&self, location: RowLocation) -> StaticFileResult<Arc<Vec<u8>>> {
        if let Some(payload) = self
            .inner
            .cache
            .lock()
            .get(&location.payload_offset)
            .cloned()
        {
            return Ok(payload);
        }
        let mut compressed = vec![
            0u8;
            usize::try_from(location.compressed_len).map_err(|_| {
                StaticFileError::invalid(
                    location.payload_offset,
                    "compressed length does not fit usize",
                )
            })?
        ];
        read_exact_at(&self.inner.file, location.payload_offset, &mut compressed).map_err(
            |source| StaticFileError::io("read compressed frame", &self.inner.path, source),
        )?;
        let payload = crate::format::decode_payload(
            location.height,
            location.uncompressed_len,
            location.payload_checksum,
            &compressed,
        )?;
        let payload = Arc::new(payload);
        self.inner
            .cache
            .lock()
            .put(location.payload_offset, Arc::clone(&payload));
        Ok(payload)
    }
}

impl StaticFileProvider for StaticFileArchive {
    fn tip(&self) -> Option<u32> {
        self.inner.index.tip()
    }

    fn get(&self, key: &[u8]) -> StaticFileResult<Option<Vec<u8>>> {
        let Some(location) = self.inner.index.row(key)? else {
            return Ok(None);
        };
        let payload = self.load_payload(location)?;
        let start = usize::try_from(location.value_offset).map_err(|_| {
            StaticFileError::invalid(location.payload_offset, "value offset does not fit usize")
        })?;
        let len = usize::try_from(location.value_len).map_err(|_| {
            StaticFileError::invalid(location.payload_offset, "value length does not fit usize")
        })?;
        let end = start.checked_add(len).ok_or_else(|| {
            StaticFileError::invalid(location.payload_offset, "value range overflow")
        })?;
        let value = payload.get(start..end).ok_or_else(|| {
            StaticFileError::invalid(
                location.payload_offset,
                "indexed value lies outside decompressed frame",
            )
        })?;
        Ok(Some(value.to_vec()))
    }
}

pub(super) struct ArchiveInner {
    pub(super) file: File,
    pub(super) path: PathBuf,
    pub(super) config: StaticFileConfig,
    pub(super) write_lock: Mutex<()>,
    pub(super) pending: Mutex<Option<PendingAppend>>,
    pub(super) index: ArchiveIndex,
    pub(super) cache: Mutex<LruCache<u64, Arc<Vec<u8>>>>,
    pub(super) healthy: AtomicBool,
}

pub(super) struct PendingAppend {
    frames: Vec<PositionedEncodedFrame>,
}

fn validate_continuity(tip: Option<u32>, records: &[StaticRecord]) -> StaticFileResult<()> {
    let mut expected = match tip {
        Some(height) => height.checked_add(1).ok_or_else(|| {
            StaticFileError::invalid(0, "cannot append after maximum block height")
        })?,
        None => 0,
    };
    for record in records {
        if record.height() != expected {
            return Err(StaticFileError::NonContiguous {
                expected,
                actual: record.height(),
            });
        }
        expected = expected.checked_add(1).unwrap_or(expected);
    }
    Ok(())
}

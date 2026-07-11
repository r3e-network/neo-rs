//! Cloneable archive provider and append/read/truncate operations.

use std::fmt;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lru::LruCache;
use parking_lot::{Mutex, RwLock};

use super::StaticFileProvider;
use super::config::StaticFileConfig;
use super::index::{ArchiveIndex, RowLocation, scan_archive};
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
    /// Appends and durably flushes one finalized-height record.
    pub fn append(&self, record: StaticRecord) -> StaticFileResult<()> {
        self.append_batch(vec![record])
    }

    /// Appends a contiguous record batch and performs one durability sync.
    pub fn append_batch(&self, records: Vec<StaticRecord>) -> StaticFileResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        if !self.is_healthy() {
            return Err(StaticFileError::Unhealthy);
        }
        let _write_guard = self.inner.write_lock.lock();
        if !self.is_healthy() {
            return Err(StaticFileError::Unhealthy);
        }

        validate_continuity(self.tip(), &records)?;
        let mut encoded = Vec::with_capacity(records.len());
        for record in records {
            encoded.push(encode_frame(record, self.inner.config)?);
        }

        let start = self.inner.index.read().file_len;
        let mut offset = start;
        for frame in &encoded {
            if let Err(source) = write_all_at(&self.inner.file, offset, &frame.bytes) {
                self.inner.healthy.store(false, Ordering::Release);
                return Err(StaticFileError::io(
                    "append frame",
                    &self.inner.path,
                    source,
                ));
            }
            offset = offset
                .checked_add(frame.header.frame_len)
                .ok_or_else(|| StaticFileError::invalid(start, "archive offset overflow"))?;
        }
        if let Err(source) = self.inner.file.sync_data() {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(StaticFileError::io(
                "sync appended frames",
                &self.inner.path,
                source,
            ));
        }

        let mut index = self.inner.index.write();
        let mut frame_offset = start;
        for frame in encoded {
            index.insert_encoded_frame(frame_offset, &frame);
            frame_offset += frame.header.frame_len;
        }
        index.file_len = offset;
        Ok(())
    }

    /// Truncates every frame above `height` and rebuilds the key index.
    ///
    /// Passing `None` resets the archive to its versioned file header. Passing
    /// a height at or above the current tip is a no-op.
    pub fn truncate_after(&self, height: Option<u32>) -> StaticFileResult<()> {
        let _write_guard = self.inner.write_lock.lock();
        let current_len = self.inner.index.read().file_len;
        let target_len = {
            let index = self.inner.index.read();
            match height {
                None => u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
                Some(target) if index.tip().is_none_or(|tip| target >= tip) => current_len,
                Some(target) => index.frames.range(..=target).next_back().map_or(
                    u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
                    |(_, frame)| frame.end,
                ),
            }
        };
        if target_len == current_len {
            return Ok(());
        }

        self.inner
            .file
            .set_len(target_len)
            .map_err(|source| StaticFileError::io("truncate", &self.inner.path, source))?;
        self.inner
            .file
            .sync_all()
            .map_err(|source| StaticFileError::io("sync truncation", &self.inner.path, source))?;
        let scanned = scan_archive(
            &self.inner.file,
            &self.inner.path,
            self.inner.config,
            target_len,
        )?;
        *self.inner.index.write() = scanned.index;
        self.inner.cache.lock().clear();
        self.inner.healthy.store(true, Ordering::Release);
        Ok(())
    }

    /// Returns the archive path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.path
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
        self.inner.index.read().tip()
    }

    fn get(&self, key: &[u8]) -> StaticFileResult<Option<Vec<u8>>> {
        let Some(location) = self.inner.index.read().rows.get(key).copied() else {
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
    pub(super) index: RwLock<ArchiveIndex>,
    pub(super) cache: Mutex<LruCache<u64, Arc<Vec<u8>>>>,
    pub(super) healthy: AtomicBool,
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

//! Cloneable archive provider and staged publication/read/truncate operations.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use lru::LruCache;
use parking_lot::{Mutex, RwLock};

use super::StaticFileProvider;
use super::config::StaticFileConfig;
use super::index::{ArchiveIndex, PositionedEncodedFrame, RowLocation, read_frame_index};
use super::io::{read_exact_at, write_all_at};
use super::segments::{ArchiveSegment, ArchiveSegments};

use crate::format::{FILE_HEADER_LEN, encode_frame};
use crate::{StaticFileError, StaticFileResult, StaticRecord};

/// Cloneable append/read handle over one height-segmented static archive.
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
            .field("segments", &self.segment_count())
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

        let state =
            self.inner.index.state().ok_or_else(|| {
                StaticFileError::invalid_index("archive index is not initialized")
            })?;
        let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
        let mut segment_start = state.active_segment_start;
        let mut offset = state.indexed_file_len;
        let mut segments = self.inner.segments.write();
        let mut segment = segments.exact(segment_start)?;
        let mut segment_dirty = false;
        let mut frames = Vec::with_capacity(encoded.len());
        for frame in encoded {
            let frame_end = offset.checked_add(frame.header.frame_len);
            if offset > header_len
                && frame_end.is_none_or(|end| end > self.inner.config.max_segment_bytes)
            {
                if segment_dirty {
                    self.sync_staged_segment(&segment)?;
                }
                segment_start = frame.header.height;
                segment = match segments.create(segment_start) {
                    Ok(segment) => segment,
                    Err(error) => {
                        self.inner.healthy.store(false, Ordering::Release);
                        return Err(error);
                    }
                };
                offset = header_len;
            }
            let end = offset
                .checked_add(frame.header.frame_len)
                .ok_or_else(|| StaticFileError::invalid(offset, "archive offset overflow"))?;
            if let Err(source) = write_all_at(&segment.file, offset, &frame.bytes) {
                self.inner.healthy.store(false, Ordering::Release);
                return Err(StaticFileError::io(
                    "stage archive frame",
                    &segment.path,
                    source,
                ));
            }
            frames.push(PositionedEncodedFrame::new(segment_start, offset, frame));
            offset = end;
            segment_dirty = true;
        }
        if segment_dirty {
            self.sync_staged_segment(&segment)?;
        }
        *self.inner.pending.lock() = Some(PendingAppend { frames });
        Ok(())
    }

    fn sync_staged_segment(&self, segment: &ArchiveSegment) -> StaticFileResult<()> {
        if let Err(source) = segment.file.sync_data() {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(StaticFileError::io(
                "sync staged archive frames",
                &segment.path,
                source,
            ));
        }
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

    /// Returns the number of immutable-prefix files in this archive.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.inner.segments.read().count()
    }

    /// Returns archive segment paths in ascending starting-height order.
    #[must_use]
    pub fn segment_paths(&self) -> Vec<PathBuf> {
        self.inner.segments.read().paths()
    }

    /// Returns whether no append durability failure has poisoned this handle.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.inner.healthy.load(Ordering::Acquire)
    }

    fn load_payload(&self, location: RowLocation) -> StaticFileResult<Arc<Vec<u8>>> {
        let segment = self.inner.segments.read().exact(location.segment_start)?;
        let cache_key = (segment.start_height, location.payload_offset);
        if let Some(payload) = self.inner.cache.lock().get(&cache_key).cloned() {
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
        read_exact_at(&segment.file, location.payload_offset, &mut compressed).map_err(
            |source| StaticFileError::io("read compressed frame", &segment.path, source),
        )?;
        let payload = crate::format::decode_payload(
            location.height,
            location.uncompressed_len,
            location.payload_checksum,
            &compressed,
        )?;
        let payload = Arc::new(payload);
        self.inner.cache.lock().put(cache_key, Arc::clone(&payload));
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

    fn frame_row_keys(&self, height: u32) -> StaticFileResult<Option<Vec<Vec<u8>>>> {
        let Some(location) = self.inner.index.frame(height)? else {
            return Ok(None);
        };
        let segment = self.inner.segments.read().exact(location.segment_start)?;
        let frame = read_frame_index(
            &segment.file,
            &segment.path,
            segment.start_height,
            self.inner.config,
            location,
        )?;
        Ok(Some(
            frame
                .rows
                .into_iter()
                .map(|row| row.key.into_vec())
                .collect(),
        ))
    }

    fn latest_heights_for_keys<K: AsRef<[u8]>>(
        &self,
        keys: &[K],
    ) -> StaticFileResult<Vec<Option<u32>>> {
        self.inner.index.latest_heights_for_keys(keys)
    }
}

pub(super) struct ArchiveInner {
    pub(super) path: PathBuf,
    pub(super) config: StaticFileConfig,
    pub(super) segments: RwLock<ArchiveSegments>,
    pub(super) write_lock: Mutex<()>,
    pub(super) pending: Mutex<Option<PendingAppend>>,
    pub(super) index: ArchiveIndex,
    pub(super) cache: Mutex<LruCache<(u32, u64), Arc<Vec<u8>>>>,
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

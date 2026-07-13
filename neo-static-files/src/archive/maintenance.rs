//! Cross-segment rollback and explicit archive/index parity scrubbing.

use std::sync::atomic::Ordering;

use super::index::{ScanMode, ScannedFrame, read_frame_index, scan_archive};
use super::provider::StaticFileArchive;
use crate::format::FILE_HEADER_LEN;
use crate::{StaticFileError, StaticFileResult};

impl StaticFileArchive {
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
        let mut segments = self.inner.segments.write();
        let (target_segment, target_len) = match height {
            None => (
                segments.exact(0)?,
                u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
            ),
            Some(target) => {
                let location = self.inner.index.frame(target)?.ok_or_else(|| {
                    StaticFileError::invalid_index("truncate target frame is missing")
                })?;
                let segment = segments.exact(location.segment_start)?;
                (segment, location.end)
            }
        };

        let removed = self
            .inner
            .index
            .frames_after(height)?
            .into_iter()
            .map(|frame| {
                let segment = segments.exact(frame.segment_start)?;
                read_frame_index(
                    &segment.file,
                    &segment.path,
                    segment.start_height,
                    self.inner.config,
                    frame,
                )
            })
            .collect::<StaticFileResult<Vec<_>>>()?;

        if let Err(error) = segments.remove_after(target_segment.start_height) {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(error);
        }
        if let Err(source) = target_segment.file.set_len(target_len) {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(StaticFileError::io(
                "truncate",
                &target_segment.path,
                source,
            ));
        }
        if let Err(source) = target_segment.file.sync_all() {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(StaticFileError::io(
                "sync truncation",
                &target_segment.path,
                source,
            ));
        }
        if let Err(error) =
            self.inner
                .index
                .truncate_after(height, target_segment.start_height, &removed)
        {
            self.inner.healthy.store(false, Ordering::Release);
            return Err(error);
        }
        self.inner.cache.lock().clear();
        self.inner.healthy.store(true, Ordering::Release);
        Ok(())
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
        const VERIFY_BATCH_FRAMES: usize = 1_024;
        let mut batch = Vec::<ScannedFrame>::with_capacity(VERIFY_BATCH_FRAMES);
        let segments = self.inner.segments.read().snapshots();
        let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
        let mut expected_height = 0u32;
        let mut frames_scanned = 0u64;
        let mut rows_scanned = 0u64;
        for segment in &segments {
            if segment.start_height != expected_height {
                return Err(StaticFileError::invalid_index(format!(
                    "segment starts at height {} while scrub expected {expected_height}",
                    segment.start_height
                )));
            }
            let file_len = segment.len()?;
            let outcome = scan_archive(
                &segment.file,
                &segment.path,
                segment.start_height,
                self.inner.config,
                file_len,
                header_len,
                expected_height,
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
            if outcome.valid_file_len != file_len {
                return Err(StaticFileError::invalid_index(
                    "strict scrub did not consume an entire archive segment",
                ));
            }
            let scanned = u32::try_from(outcome.frames_scanned).map_err(|_| {
                StaticFileError::invalid_index("segment frame count does not fit u32")
            })?;
            expected_height = expected_height.checked_add(scanned).ok_or_else(|| {
                StaticFileError::invalid_index("archive height overflow during scrub")
            })?;
            frames_scanned = frames_scanned.saturating_add(outcome.frames_scanned);
            rows_scanned = rows_scanned.saturating_add(outcome.rows_scanned);
        }
        if !batch.is_empty() {
            self.inner.index.verify_frames(&batch)?;
        }
        let expected_frames = state.tip.map_or(0, |tip| u64::from(tip) + 1);
        let active_segment = segments
            .last()
            .ok_or_else(|| StaticFileError::invalid_index("archive has no genesis segment"))?;
        if active_segment.start_height != state.active_segment_start
            || active_segment.len()? != state.indexed_file_len
            || frames_scanned != expected_frames
            || self.inner.index.stored_frame_count()? != expected_frames
            || rows_scanned != state.row_versions
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
            .insert_test_frame(super::index::FrameLocation {
                height,
                segment_start: 0,
                start,
                end,
            })
    }
}

//! Frame-range lookup and version rollback for archive truncation.

use libmdbx::WriteFlags;

use super::{ArchiveIndex, FRAMES_TABLE, META_TABLE, ROWS_TABLE, STATE_KEY};
use crate::archive::index::{FrameLocation, IndexState, RowLocation, ScannedFrame};
use crate::{StaticFileError, StaticFileResult};

impl ArchiveIndex {
    pub(crate) fn frames_after(&self, height: Option<u32>) -> StaticFileResult<Vec<FrameLocation>> {
        let first_height = match height {
            None => 0,
            Some(height) => match height.checked_add(1) {
                Some(height) => height,
                None => return Ok(Vec::new()),
            },
        };
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin frame range", error))?;
        let table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        let mut cursor = tx
            .cursor(&table)
            .map_err(|error| self.error("open frame cursor", error))?;
        let mut entry = cursor
            .set_range::<Vec<u8>, Vec<u8>>(&first_height.to_be_bytes())
            .map_err(|error| self.error("seek frame range", error))?;
        let mut frames = Vec::new();
        while let Some((key, value)) = entry {
            let height = decode_height_key(&key)?;
            frames.push(FrameLocation::decode(height, &value)?);
            entry = cursor
                .next::<Vec<u8>, Vec<u8>>()
                .map_err(|error| self.error("advance frame cursor", error))?;
        }
        Ok(frames)
    }

    pub(crate) fn truncate_after(
        &self,
        height: Option<u32>,
        removed: &[ScannedFrame],
    ) -> StaticFileResult<IndexState> {
        let Some(current) = self.state() else {
            return Err(StaticFileError::invalid_index(
                "cannot truncate an uninitialized index",
            ));
        };
        if removed.is_empty() {
            return Ok(current);
        }
        let removed_rows = removed.iter().try_fold(0u64, |total, frame| {
            total
                .checked_add(u64::try_from(frame.rows.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| StaticFileError::invalid_index("removed row count overflow"))
        })?;
        let state = match height {
            Some(height) => {
                let frame = self.frame(height)?.ok_or_else(|| {
                    StaticFileError::invalid_index("truncate target frame is missing")
                })?;
                IndexState {
                    archive_id: current.archive_id,
                    indexed_file_len: frame.end,
                    tip: Some(height),
                    last_frame_start: frame.start,
                    row_versions: current.row_versions.checked_sub(removed_rows).ok_or_else(
                        || StaticFileError::invalid_index("row-version count underflow"),
                    )?,
                    tail_recovery_safe: current.tail_recovery_safe,
                }
            }
            None => IndexState::empty(current.archive_id, current.tail_recovery_safe),
        };

        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin truncation", error))?;
        let meta = tx
            .open_table(Some(META_TABLE))
            .map_err(|error| self.error("open metadata table", error))?;
        let frame_table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        let row_table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;
        for frame in removed {
            for row in &frame.rows {
                let location = RowLocation::from_frame(frame.offset, frame.header, row);
                let encoded = location.encode(&row.key);
                let deleted = tx
                    .del(&row_table, &row.key, Some(&encoded))
                    .map_err(|error| self.error("remove row version", error))?;
                if !deleted {
                    return Err(StaticFileError::invalid_index(
                        "truncated row version is missing",
                    ));
                }
            }
            let deleted = tx
                .del(&frame_table, frame.header.height.to_be_bytes(), None)
                .map_err(|error| self.error("remove frame location", error))?;
            if !deleted {
                return Err(StaticFileError::invalid_index(
                    "truncated frame location is missing",
                ));
            }
        }
        tx.put(&meta, STATE_KEY, state.encode(), WriteFlags::UPSERT)
            .map_err(|error| self.error("publish truncated state", error))?;
        tx.commit()
            .map_err(|error| self.error("commit truncation", error))?;
        *self.state.write() = Some(state);
        Ok(state)
    }
}

fn decode_height_key(bytes: &[u8]) -> StaticFileResult<u32> {
    let key: [u8; 4] = bytes
        .try_into()
        .map_err(|_| StaticFileError::invalid_index("frame key length mismatch"))?;
    Ok(u32::from_be_bytes(key))
}

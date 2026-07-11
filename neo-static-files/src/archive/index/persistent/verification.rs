//! Full archive-to-index parity checks used by explicit scrubbing.

#[cfg(test)]
use libmdbx::WriteFlags;

use super::{ArchiveIndex, FRAMES_TABLE, ROWS_TABLE};
use crate::archive::index::{FrameLocation, RowLocation, ScannedFrame};
use crate::{StaticFileError, StaticFileResult};

impl ArchiveIndex {
    pub(crate) fn verify_frames(&self, frames: &[ScannedFrame]) -> StaticFileResult<()> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin index verification", error))?;
        let frame_table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        let row_table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;
        let mut row_cursor = tx
            .cursor(&row_table)
            .map_err(|error| self.error("open row cursor", error))?;

        for frame in frames {
            let expected_frame = FrameLocation::from_frame(frame.offset, frame.header)?.encode();
            let actual_frame = tx
                .get::<Vec<u8>>(&frame_table, &frame.header.height.to_be_bytes())
                .map_err(|error| self.error("verify frame location", error))?
                .ok_or_else(|| StaticFileError::invalid_index("scrub found a missing frame"))?;
            if actual_frame != expected_frame {
                return Err(StaticFileError::invalid_index(
                    "scrub found a mismatched frame location",
                ));
            }
            for row in &frame.rows {
                let expected =
                    RowLocation::from_frame(frame.offset, frame.header, row).encode(&row.key);
                if row_cursor
                    .get_both::<Vec<u8>>(&row.key, &expected)
                    .map_err(|error| self.error("verify row location", error))?
                    .is_none()
                {
                    return Err(StaticFileError::invalid_index(
                        "scrub found a missing row location",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(crate) fn stored_row_versions(&self) -> StaticFileResult<u64> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin row count", error))?;
        let table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;
        u64::try_from(
            tx.table_stat(&table)
                .map_err(|error| self.error("read row table statistics", error))?
                .entries(),
        )
        .map_err(|_| StaticFileError::invalid_index("row count does not fit u64"))
    }

    pub(crate) fn stored_frame_count(&self) -> StaticFileResult<u64> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin frame count", error))?;
        let table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        u64::try_from(
            tx.table_stat(&table)
                .map_err(|error| self.error("read frame table statistics", error))?
                .entries(),
        )
        .map_err(|_| StaticFileError::invalid_index("frame count does not fit u64"))
    }

    #[cfg(test)]
    pub(crate) fn insert_test_frame(&self, frame: FrameLocation) -> StaticFileResult<()> {
        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin test frame insertion", error))?;
        let table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        tx.put(
            &table,
            frame.height.to_be_bytes(),
            frame.encode(),
            WriteFlags::UPSERT,
        )
        .map_err(|error| self.error("insert test frame", error))?;
        tx.commit()
            .map_err(|error| self.error("commit test frame insertion", error))?;
        Ok(())
    }
}

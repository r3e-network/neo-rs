//! # Persistent archive index
//!
//! ## Boundary
//!
//! MDBX stores derived frame offsets and versioned row locations. Archive
//! bytes remain authoritative and can rebuild this entire index.
//!
//! ## Contents
//!
//! - Core open, reset, publication, and point-lookup operations.
//! - `recovery`: Range lookup and rollback after archive truncation.
//! - `verification`: Strict archive-to-index parity checks.

mod recovery;
mod verification;

use std::fs;
use std::path::{Path, PathBuf};

use libmdbx::{
    Database, DatabaseOptions, Mode, NoWriteMap, ReadWriteOptions, SyncMode, TableFlags, WriteFlags,
};
use parking_lot::RwLock;

use super::model::{FrameLocation, IndexState, IndexedFrame, RowLocation};
use crate::{StaticFileError, StaticFileResult};

const META_TABLE: &str = "meta";
const FRAMES_TABLE: &str = "frames";
const ROWS_TABLE: &str = "rows";
const STATE_KEY: &[u8] = b"state";
const MAX_INDEX_BYTES: u64 = 1 << 40;
const INDEX_GROWTH_BYTES: u64 = 64 << 20;

/// Persistent, derived lookup state for one authoritative static archive.
pub(crate) struct ArchiveIndex {
    db: Database<NoWriteMap>,
    path: PathBuf,
    state: RwLock<Option<IndexState>>,
}

impl ArchiveIndex {
    pub(crate) fn open(archive_path: &Path) -> StaticFileResult<Self> {
        let path = index_path_for(archive_path);
        fs::create_dir_all(&path)
            .map_err(|source| StaticFileError::io("create index directory", &path, source))?;
        let db = Database::<NoWriteMap>::open_with_options(
            &path,
            DatabaseOptions {
                max_tables: Some(4),
                no_rdahead: true,
                coalesce: true,
                liforeclaim: true,
                mode: Mode::ReadWrite(ReadWriteOptions {
                    sync_mode: SyncMode::Durable,
                    max_size: isize::try_from(MAX_INDEX_BYTES).ok(),
                    growth_step: isize::try_from(INDEX_GROWTH_BYTES).ok(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .map_err(|error| StaticFileError::index("open", &path, error.to_string()))?;

        let index = Self {
            db,
            path,
            state: RwLock::new(None),
        };
        index.create_tables()?;
        *index.state.write() = match index.read_state() {
            Ok(state) => state,
            Err(StaticFileError::InvalidFormat { .. }) => None,
            Err(error) => return Err(error),
        };
        Ok(index)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn state(&self) -> Option<IndexState> {
        *self.state.read()
    }

    pub(crate) fn tip(&self) -> Option<u32> {
        self.state().and_then(|state| state.tip)
    }

    pub(crate) fn reset(
        &self,
        archive_id: u64,
        tail_recovery_safe: bool,
    ) -> StaticFileResult<IndexState> {
        let state = IndexState::empty(archive_id, tail_recovery_safe);
        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin reset", error))?;
        let meta = tx
            .create_table(Some(META_TABLE), TableFlags::empty())
            .map_err(|error| self.error("open metadata table", error))?;
        let frames = tx
            .create_table(Some(FRAMES_TABLE), TableFlags::empty())
            .map_err(|error| self.error("open frame table", error))?;
        let rows = tx
            .create_table(
                Some(ROWS_TABLE),
                TableFlags::DUP_SORT | TableFlags::DUP_FIXED,
            )
            .map_err(|error| self.error("open row table", error))?;
        tx.clear_table(&meta)
            .map_err(|error| self.error("clear metadata table", error))?;
        tx.clear_table(&frames)
            .map_err(|error| self.error("clear frame table", error))?;
        tx.clear_table(&rows)
            .map_err(|error| self.error("clear row table", error))?;
        tx.put(&meta, STATE_KEY, state.encode(), WriteFlags::UPSERT)
            .map_err(|error| self.error("write reset state", error))?;
        tx.commit()
            .map_err(|error| self.error("commit reset", error))?;
        *self.state.write() = Some(state);
        Ok(state)
    }

    pub(crate) fn mark_tail_recovery_safe(&self) -> StaticFileResult<IndexState> {
        let Some(mut state) = self.state() else {
            return Err(StaticFileError::invalid_index(
                "cannot finalize an uninitialized index rebuild",
            ));
        };
        if state.tail_recovery_safe {
            return Ok(state);
        }
        state.tail_recovery_safe = true;
        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin rebuild finalization", error))?;
        let meta = tx
            .open_table(Some(META_TABLE))
            .map_err(|error| self.error("open metadata table", error))?;
        tx.put(&meta, STATE_KEY, state.encode(), WriteFlags::UPSERT)
            .map_err(|error| self.error("finalize rebuild state", error))?;
        tx.commit()
            .map_err(|error| self.error("commit rebuild finalization", error))?;
        *self.state.write() = Some(state);
        Ok(state)
    }

    pub(crate) fn publish_frames<F>(&self, frames: &[F]) -> StaticFileResult<IndexState>
    where
        F: IndexedFrame,
    {
        let Some(mut state) = self.state() else {
            return Err(StaticFileError::invalid_index(
                "cannot publish frames before index initialization",
            ));
        };
        if frames.is_empty() {
            return Ok(state);
        }

        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin publication", error))?;
        let meta = tx
            .open_table(Some(META_TABLE))
            .map_err(|error| self.error("open metadata table", error))?;
        let frame_table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        let row_table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;

        for frame in frames {
            state.advance(frame)?;
            let header = frame.header();
            let location =
                FrameLocation::from_frame(frame.segment_start(), frame.offset(), header)?;
            tx.put(
                &frame_table,
                header.height.to_be_bytes(),
                location.encode(),
                WriteFlags::UPSERT,
            )
            .map_err(|error| self.error("publish frame location", error))?;
            for row in frame.rows() {
                let location =
                    RowLocation::from_frame(frame.segment_start(), frame.offset(), header, row);
                tx.put(
                    &row_table,
                    &row.key,
                    location.encode(&row.key),
                    WriteFlags::UPSERT,
                )
                .map_err(|error| self.error("publish row location", error))?;
            }
        }
        tx.put(&meta, STATE_KEY, state.encode(), WriteFlags::UPSERT)
            .map_err(|error| self.error("publish index state", error))?;
        tx.commit()
            .map_err(|error| self.error("commit publication", error))?;
        *self.state.write() = Some(state);
        Ok(state)
    }

    pub(crate) fn row(&self, key: &[u8]) -> StaticFileResult<Option<RowLocation>> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin row lookup", error))?;
        let table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;
        let mut cursor = tx
            .cursor(&table)
            .map_err(|error| self.error("open row cursor", error))?;
        if cursor
            .set::<Vec<u8>>(key)
            .map_err(|error| self.error("seek row", error))?
            .is_none()
        {
            return Ok(None);
        }
        let bytes = cursor
            .last_dup::<Vec<u8>>()
            .map_err(|error| self.error("read latest row", error))?
            .ok_or_else(|| StaticFileError::invalid_index("row key has no location value"))?;
        RowLocation::decode(key, &bytes).map(Some)
    }

    pub(crate) fn latest_heights_for_keys<K: AsRef<[u8]>>(
        &self,
        keys: &[K],
    ) -> StaticFileResult<Vec<Option<u32>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin batched row lookup", error))?;
        let table = tx
            .open_table(Some(ROWS_TABLE))
            .map_err(|error| self.error("open row table", error))?;
        let mut cursor = tx
            .cursor(&table)
            .map_err(|error| self.error("open row cursor", error))?;
        let mut heights = Vec::with_capacity(keys.len());

        for key in keys {
            let key = key.as_ref();
            if cursor
                .set::<Vec<u8>>(key)
                .map_err(|error| self.error("seek row", error))?
                .is_none()
            {
                heights.push(None);
                continue;
            }
            let bytes = cursor
                .last_dup::<Vec<u8>>()
                .map_err(|error| self.error("read latest row", error))?
                .ok_or_else(|| StaticFileError::invalid_index("row key has no location value"))?;
            heights.push(Some(RowLocation::decode(key, &bytes)?.height));
        }

        Ok(heights)
    }

    pub(crate) fn frame(&self, height: u32) -> StaticFileResult<Option<FrameLocation>> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin frame lookup", error))?;
        let table = tx
            .open_table(Some(FRAMES_TABLE))
            .map_err(|error| self.error("open frame table", error))?;
        tx.get::<Vec<u8>>(&table, &height.to_be_bytes())
            .map_err(|error| self.error("read frame location", error))?
            .map(|bytes| FrameLocation::decode(height, &bytes))
            .transpose()
    }

    fn create_tables(&self) -> StaticFileResult<()> {
        let tx = self
            .db
            .begin_rw_txn()
            .map_err(|error| self.error("begin initialization", error))?;
        tx.create_table(Some(META_TABLE), TableFlags::empty())
            .map_err(|error| self.error("create metadata table", error))?;
        tx.create_table(Some(FRAMES_TABLE), TableFlags::empty())
            .map_err(|error| self.error("create frame table", error))?;
        tx.create_table(
            Some(ROWS_TABLE),
            TableFlags::DUP_SORT | TableFlags::DUP_FIXED,
        )
        .map_err(|error| self.error("create row table", error))?;
        tx.commit()
            .map_err(|error| self.error("commit initialization", error))?;
        Ok(())
    }

    fn read_state(&self) -> StaticFileResult<Option<IndexState>> {
        let tx = self
            .db
            .begin_ro_txn()
            .map_err(|error| self.error("begin state read", error))?;
        let table = tx
            .open_table(Some(META_TABLE))
            .map_err(|error| self.error("open metadata table", error))?;
        tx.get::<Vec<u8>>(&table, STATE_KEY)
            .map_err(|error| self.error("read index state", error))?
            .map(|bytes| IndexState::decode(&bytes))
            .transpose()
    }

    fn error(&self, operation: &'static str, error: libmdbx::Error) -> StaticFileError {
        StaticFileError::index(operation, &self.path, error.to_string())
    }
}

fn index_path_for(archive_path: &Path) -> PathBuf {
    let mut path = archive_path.as_os_str().to_os_string();
    path.push(".index");
    PathBuf::from(path)
}

#![allow(unsafe_code)]

use super::snapshot::MdbxSnapshot;
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{MdbxEnvironmentInfo, RawOverlaySource, Store, StoreBackendKind},
    write_store::WriteStore,
};
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
use libmdbx::{
    Cursor, Database, DatabaseOptions, Error as MdbxError, Mode, NoWriteMap, RO, ReadWriteOptions,
    SyncMode, TableFlags, Transaction, TransactionKind, WriteFlags,
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
use tracing::{error, warn};

type RawEntry = (Vec<u8>, Vec<u8>);

/// Persistent MDBX implementation of the Neo storage traits.
pub struct MdbxStore {
    db: Arc<Database<NoWriteMap>>,
}

impl std::fmt::Debug for MdbxStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdbxStore").finish_non_exhaustive()
    }
}

impl MdbxStore {
    pub(crate) fn open(
        path: &Path,
        max_size: isize,
        growth_step: isize,
        max_readers: u32,
        read_only: bool,
    ) -> StorageResult<Self> {
        if !read_only {
            fs::create_dir_all(path).map_err(|err| StorageError::Io {
                message: format!(
                    "failed to create MDBX data directory {}: {err}",
                    path.display()
                ),
            })?;
        }

        let db = Database::<NoWriteMap>::open_with_options(
            path,
            DatabaseOptions {
                // libmdbx 0.5.x exposes this option but does not forward it to
                // MDBX before open; keep the field here so a wrapper upgrade
                // starts enforcing it without changing the provider boundary.
                max_readers: Some(max_readers),
                mode: if read_only {
                    Mode::ReadOnly
                } else {
                    Mode::ReadWrite(ReadWriteOptions {
                        sync_mode: SyncMode::Durable,
                        max_size: Some(max_size),
                        growth_step: Some(growth_step),
                        ..Default::default()
                    })
                },
                ..Default::default()
            },
        )
        .map_err(|err| StorageError::Io {
            message: format!("failed to open MDBX store at {}: {err}", path.display()),
        })?;

        if !read_only {
            let tx = db.begin_rw_txn().map_err(mdbx_error)?;
            tx.create_table(None, TableFlags::empty())
                .map_err(mdbx_error)?;
            tx.commit().map_err(mdbx_commit_error)?;
        }

        Ok(Self { db: Arc::new(db) })
    }

    fn read_entry(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(None).map_err(mdbx_error)?;
        tx.get::<Vec<u8>>(&table, key).map_err(mdbx_error)
    }

    fn collect_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> StorageResult<Vec<RawEntry>> {
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(None).map_err(mdbx_error)?;
        let mut cursor = tx.cursor(&table).map_err(mdbx_error)?;
        collect_cursor_entries(&mut cursor, key_prefix, direction)
    }

    pub(crate) fn read_txn(&self) -> StorageResult<Transaction<'static, RO, NoWriteMap>> {
        let db_ptr = Arc::into_raw(Arc::clone(&self.db));
        let tx = unsafe {
            // SAFETY: the returned snapshot owns `self.clone()`, which owns an
            // `Arc<Database<NoWriteMap>>`. That Arc keeps the database alive
            // for at least as long as the transaction field is dropped.
            let db: &'static Database<NoWriteMap> = &*db_ptr;
            db.begin_ro_txn().map_err(mdbx_error)?
        };
        unsafe {
            // Balance the temporary Arc clone created for the widened borrow.
            drop(Arc::from_raw(db_ptr));
        }
        Ok(tx)
    }

    /// Returns current MDBX environment information for diagnostics and tests.
    pub fn info(&self) -> StorageResult<libmdbx::Info> {
        self.db.info().map_err(mdbx_error)
    }

    /// Commits raw byte-key overlay entries directly to MDBX.
    pub fn commit_raw_overlay<'a, I>(&self, overlay: I) -> StorageResult<()>
    where
        I: IntoIterator<Item = (&'a [u8], Option<&'a [u8]>)>,
    {
        let mut entries = overlay.into_iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        if entries.is_empty() {
            return Ok(());
        }

        let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
        let table = tx
            .create_table(None, TableFlags::empty())
            .map_err(mdbx_error)?;
        for (key, value) in entries {
            match value {
                Some(value) => tx
                    .put(&table, key, value, WriteFlags::UPSERT)
                    .map_err(mdbx_error)?,
                None => {
                    tx.del(&table, key, None).map_err(mdbx_error)?;
                }
            }
        }

        tx.commit().map_err(mdbx_commit_error)?;
        Ok(())
    }

    pub(crate) fn commit_overlay(
        &self,
        overlay: &BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    ) -> StorageResult<()> {
        self.commit_raw_overlay(
            overlay
                .iter()
                .map(|(key, value)| (key.as_slice(), value.as_deref())),
        )
    }
}

pub(crate) fn collect_cursor_entries<K>(
    cursor: &mut Cursor<'_, K>,
    key_prefix: Option<&[u8]>,
    direction: SeekDirection,
) -> StorageResult<Vec<RawEntry>>
where
    K: TransactionKind,
{
    match direction {
        SeekDirection::Forward => collect_cursor_entries_forward(cursor, key_prefix),
        SeekDirection::Backward => collect_cursor_entries_backward(cursor, key_prefix),
    }
}

fn collect_cursor_entries_forward<K>(
    cursor: &mut Cursor<'_, K>,
    key_prefix: Option<&[u8]>,
) -> StorageResult<Vec<RawEntry>>
where
    K: TransactionKind,
{
    let mut entry = match key_prefix {
        Some(prefix) if !prefix.is_empty() => cursor
            .set_range::<Vec<u8>, Vec<u8>>(prefix)
            .map_err(mdbx_error)?,
        _ => cursor.first::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?,
    };
    let mut entries = Vec::new();

    while let Some((key, value)) = entry {
        if !matches_prefix(&key, key_prefix) {
            break;
        }
        entries.push((key, value));
        entry = cursor.next::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?;
    }

    Ok(entries)
}

fn collect_cursor_entries_backward<K>(
    cursor: &mut Cursor<'_, K>,
    key_prefix: Option<&[u8]>,
) -> StorageResult<Vec<RawEntry>>
where
    K: TransactionKind,
{
    let mut entry = match key_prefix {
        Some(prefix) if !prefix.is_empty() => {
            let upper_bound = prefix_upper_bound(prefix);
            match upper_bound.as_deref() {
                Some(upper_bound) => {
                    if cursor
                        .set_range::<Vec<u8>, Vec<u8>>(upper_bound)
                        .map_err(mdbx_error)?
                        .is_some()
                    {
                        cursor.prev::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?
                    } else {
                        cursor.last::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?
                    }
                }
                None => cursor.last::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?,
            }
        }
        _ => cursor.last::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?,
    };
    let mut entries = Vec::new();

    while let Some((key, value)) = entry {
        if !matches_prefix(&key, key_prefix) {
            break;
        }
        entries.push((key, value));
        entry = cursor.prev::<Vec<u8>, Vec<u8>>().map_err(mdbx_error)?;
    }

    Ok(entries)
}

fn matches_prefix(key: &[u8], key_prefix: Option<&[u8]>) -> bool {
    key_prefix.is_none_or(|prefix| key.starts_with(prefix))
}

fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut upper_bound = prefix.to_vec();
    for index in (0..upper_bound.len()).rev() {
        if upper_bound[index] != u8::MAX {
            upper_bound[index] += 1;
            upper_bound.truncate(index + 1);
            return Some(upper_bound);
        }
    }
    None
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MdbxStore {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self.read_entry(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        match self.collect_entries(key_prefix.map(Vec::as_slice), direction) {
            Ok(entries) => entries.into_iter(),
            Err(err) => {
                warn!(target: "neo", error = %err, "MDBX find failed");
                Vec::new().into_iter()
            }
        }
    }
}

impl RawReadOnlyStore for MdbxStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.read_entry(key) {
            Ok(value) => value,
            Err(err) => {
                warn!(target: "neo", error = %err, "MDBX get failed");
                None
            }
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for MdbxStore {
    type FindIterator<'a> = std::vec::IntoIter<(StorageKey, StorageItem)>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.try_get(&key.to_array()).map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let prefix_bytes = key_prefix.map(StorageKey::to_array);
        match self.collect_entries(prefix_bytes.as_deref(), direction) {
            Ok(entries) => entries
                .into_iter()
                .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
                .collect::<Vec<_>>()
                .into_iter(),
            Err(err) => {
                warn!(target: "neo", error = %err, "MDBX typed find failed");
                Vec::new().into_iter()
            }
        }
    }
}

impl ReadOnlyStore for MdbxStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for MdbxStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.commit_raw_overlay([(key.as_slice(), None)])
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.commit_raw_overlay([(key.as_slice(), Some(value.as_slice()))])
    }
}

impl Store for MdbxStore {
    type Snapshot = MdbxSnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        Arc::new(MdbxSnapshot::new(Arc::new(self.clone())))
    }

    fn flush(&self) -> StorageResult<()> {
        self.db.sync(true).map(|_| ()).map_err(mdbx_error)
    }

    fn backend_kind(&self) -> StoreBackendKind {
        StoreBackendKind::Mdbx
    }

    fn mdbx_environment_info(&self) -> Option<StorageResult<MdbxEnvironmentInfo>> {
        Some(self.info().map(|info| MdbxEnvironmentInfo {
            map_size: info.map_size(),
            last_pgno: info.last_pgno(),
            last_txnid: info.last_txnid(),
            max_readers: info.max_readers(),
            num_readers: info.num_readers(),
        }))
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        self.commit_raw_overlay(
            overlay
                .iter()
                .map(|(key, value)| (key.as_slice(), value.as_deref())),
        )?;
        Ok(true)
    }

    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
        let table = tx
            .create_table(None, TableFlags::empty())
            .map_err(mdbx_error)?;
        let mut has_entries = false;
        let mut apply_result = Ok(());
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            if apply_result.is_err() {
                return;
            }
            has_entries = true;
            apply_result = match value {
                Some(value) => tx
                    .put(&table, key, value, WriteFlags::UPSERT)
                    .map_err(mdbx_error),
                None => tx.del(&table, key, None).map(|_| ()).map_err(mdbx_error),
            };
        };
        overlay.visit_raw_overlay(&mut sink);
        apply_result?;

        if has_entries {
            tx.commit().map_err(mdbx_commit_error)?;
        }
        Ok(true)
    }

    fn try_commit_durable_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        self.try_commit_borrowed_raw_overlay(overlay)
    }
}

impl Clone for MdbxStore {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
        }
    }
}

pub(crate) fn mdbx_error(err: MdbxError) -> StorageError {
    let message = err.to_string();
    error!(target: "neo", error = %message, "MDBX backend error");
    StorageError::Backend { message }
}

fn mdbx_commit_error(err: MdbxError) -> StorageError {
    let message = err.to_string();
    error!(target: "neo", error = %message, "MDBX commit failed");
    StorageError::CommitFailed(format!("MDBX commit failed: {message}"))
}

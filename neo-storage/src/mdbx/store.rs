#![allow(unsafe_code)]

use super::snapshot::MdbxSnapshot;
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{MdbxEnvironmentInfo, RawOverlaySource, Store, StoreBackendKind},
    store_maintenance::StoreMaintenanceBatch,
    transactional_store::{CoordinatedTransactionalStore, TransactionalStore},
    write_store::WriteStore,
};
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
use libmdbx::{
    Cursor, Database, DatabaseOptions, Error as MdbxError, Mode, NoWriteMap, RO, RW,
    ReadWriteOptions, SyncMode, Table, TableFlags, Transaction, TransactionKind, WriteFlags,
};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
use tracing::{error, warn};

type RawEntry = (Vec<u8>, Vec<u8>);

const MAINTENANCE_TABLE: &str = "neo_node_metadata";
const MAX_TABLES: u64 = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum DataTable {
    #[default]
    Canonical,
    Named(Arc<str>),
}

impl DataTable {
    fn name(&self) -> Option<&str> {
        match self {
            Self::Canonical => None,
            Self::Named(name) => Some(name),
        }
    }
}

/// Persistent MDBX implementation of the Neo storage traits.
pub struct MdbxStore {
    db: Arc<Database<NoWriteMap>>,
    data_table: DataTable,
    read_only: bool,
}

impl std::fmt::Debug for MdbxStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdbxStore")
            .field("data_table", &self.data_table)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
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
                // Canonical Ledger, node metadata, and service-owned domains
                // share one environment while retaining collision-free tables.
                max_tables: Some(MAX_TABLES),
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
            tx.create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
                .map_err(mdbx_error)?;
            tx.commit().map_err(mdbx_commit_error)?;
        }

        Ok(Self {
            db: Arc::new(db),
            data_table: DataTable::Canonical,
            read_only,
        })
    }

    /// Opens an isolated named-table view in this MDBX environment.
    ///
    /// The returned store shares the environment, MVCC snapshots, writer lock,
    /// and durability domain with `self`, but all normal `Store` reads and writes
    /// target `name`. This is the primitive used to keep service byte formats
    /// collision-free while allowing a later canonical commit to update several
    /// domains in one MDBX transaction.
    pub fn open_named_table(&self, name: impl AsRef<str>) -> StorageResult<Self> {
        let name = name.as_ref();
        validate_data_table_name(name)?;

        if self.read_only {
            let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
            tx.open_table(Some(name)).map_err(mdbx_error)?;
        } else {
            let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
            tx.create_table(Some(name), TableFlags::empty())
                .map_err(mdbx_error)?;
            tx.commit().map_err(mdbx_commit_error)?;
        }

        Ok(Self {
            db: Arc::clone(&self.db),
            data_table: DataTable::Named(Arc::from(name)),
            read_only: self.read_only,
        })
    }

    /// Returns the logical data-table name, or `None` for the canonical table.
    pub fn data_table_name(&self) -> Option<&str> {
        self.data_table.name()
    }

    /// Returns whether both views participate in the same MDBX transaction domain.
    pub fn shares_environment_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.db, &other.db)
    }

    fn read_entry(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(self.data_table.name()).map_err(mdbx_error)?;
        tx.get::<Vec<u8>>(&table, key).map_err(mdbx_error)
    }

    fn collect_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> StorageResult<Vec<RawEntry>> {
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(self.data_table.name()).map_err(mdbx_error)?;
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
            .create_table(self.data_table.name(), TableFlags::empty())
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

    /// Atomically commits overlays belonging to two isolated views.
    ///
    /// Both stores must share this exact MDBX environment and must select
    /// different tables. Validation happens before either overlay is visited,
    /// so a mismatched domain cannot partially publish the primary overlay.
    pub fn commit_coordinated_overlays<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        if !self.shares_environment_with(secondary_store) {
            return Err(StorageError::invalid_operation(
                "coordinated MDBX commit requires stores from the same environment",
            ));
        }
        if self.data_table == secondary_store.data_table {
            return Err(StorageError::invalid_operation(
                "coordinated MDBX commit requires distinct data tables",
            ));
        }

        let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
        let primary_table = tx
            .create_table(self.data_table.name(), TableFlags::empty())
            .map_err(mdbx_error)?;
        apply_overlay(&tx, &primary_table, primary)?;

        let secondary_table = tx
            .create_table(secondary_store.data_table.name(), TableFlags::empty())
            .map_err(mdbx_error)?;
        apply_overlay(&tx, &secondary_table, secondary)?;
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

    fn commit_borrowed_overlay<O>(&self, overlay: &mut O) -> StorageResult<()>
    where
        O: RawOverlaySource + ?Sized,
    {
        let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
        let table = tx
            .create_table(self.data_table.name(), TableFlags::empty())
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
        Ok(())
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
        self.commit_borrowed_overlay(overlay)?;
        Ok(true)
    }
}

impl TransactionalStore for MdbxStore {
    fn commit_canonical_overlay<O>(&self, overlay: &mut O) -> StorageResult<()>
    where
        O: RawOverlaySource + ?Sized,
    {
        self.commit_borrowed_overlay(overlay)
    }

    fn maintenance_metadata(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = match tx.open_table(Some(MAINTENANCE_TABLE)) {
            Ok(table) => table,
            Err(MdbxError::NotFound) => return Ok(None),
            Err(error) => return Err(mdbx_error(error)),
        };
        tx.get::<Vec<u8>>(&table, key).map_err(mdbx_error)
    }

    fn commit_maintenance(&self, batch: &StoreMaintenanceBatch) -> StorageResult<()> {
        if batch.is_empty() {
            return Ok(());
        }

        let tx = self.db.begin_rw_txn().map_err(mdbx_error)?;
        let data = tx
            .create_table(self.data_table.name(), TableFlags::empty())
            .map_err(mdbx_error)?;
        let metadata = tx
            .create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
            .map_err(mdbx_error)?;
        for (key, value) in batch.data_operations() {
            match value {
                Some(value) => tx
                    .put(&data, key, value, WriteFlags::UPSERT)
                    .map_err(mdbx_error)?,
                None => {
                    tx.del(&data, key, None).map_err(mdbx_error)?;
                }
            }
        }
        for (key, value) in batch.metadata_operations() {
            match value {
                Some(value) => tx
                    .put(&metadata, key, value, WriteFlags::UPSERT)
                    .map_err(mdbx_error)?,
                None => {
                    tx.del(&metadata, key, None).map_err(mdbx_error)?;
                }
            }
        }
        tx.commit().map_err(mdbx_commit_error)?;
        Ok(())
    }
}

impl CoordinatedTransactionalStore for MdbxStore {
    fn open_namespace(&self, name: &str) -> StorageResult<Self> {
        self.open_named_table(name)
    }

    fn shares_commit_domain(&self, other: &Self) -> bool {
        self.shares_environment_with(other)
    }

    fn commit_coordinated_overlays<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        MdbxStore::commit_coordinated_overlays(self, primary, secondary_store, secondary)
    }
}

impl Clone for MdbxStore {
    fn clone(&self) -> Self {
        Self {
            db: Arc::clone(&self.db),
            data_table: self.data_table.clone(),
            read_only: self.read_only,
        }
    }
}

fn validate_data_table_name(name: &str) -> StorageResult<()> {
    if name.is_empty() {
        return Err(StorageError::invalid_operation(
            "MDBX data table name must not be empty",
        ));
    }
    if name == MAINTENANCE_TABLE {
        return Err(StorageError::invalid_operation(
            "MDBX node-maintenance table is reserved",
        ));
    }
    if name.as_bytes().contains(&0) {
        return Err(StorageError::invalid_operation(
            "MDBX data table name must not contain NUL",
        ));
    }
    Ok(())
}

fn apply_overlay<O>(
    tx: &Transaction<'_, RW, NoWriteMap>,
    table: &Table<'_>,
    overlay: &mut O,
) -> StorageResult<bool>
where
    O: RawOverlaySource + ?Sized,
{
    let mut has_entries = false;
    let mut apply_result = Ok(());
    let mut sink = |key: &[u8], value: Option<&[u8]>| {
        if apply_result.is_err() {
            return;
        }
        has_entries = true;
        apply_result = match value {
            Some(value) => tx
                .put(table, key, value, WriteFlags::UPSERT)
                .map_err(mdbx_error),
            None => tx.del(table, key, None).map(|_| ()).map_err(mdbx_error),
        };
    };
    overlay.visit_raw_overlay(&mut sink);
    apply_result?;
    Ok(has_entries)
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

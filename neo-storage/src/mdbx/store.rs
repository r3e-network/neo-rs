#![allow(unsafe_code)]

use super::metrics::{MdbxCommitCountKind, MdbxCommitRecorder, MdbxCommitStage, elapsed_us};
use super::prefix_occupancy::{
    PrefixOccupancyBuildReport, PrefixOccupancyBuilder, PrefixOccupancyIndex, PrefixOccupancySpec,
};
use super::snapshot::MdbxSnapshot;
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{MdbxEnvironmentInfo, RawOverlaySink, RawOverlaySource, Store, StoreBackendKind},
    store_maintenance::StoreMaintenanceBatch,
    transactional_store::{CoordinatedTransactionalStore, TransactionalStore},
    write_store::WriteStore,
};
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
use libmdbx::{
    Cursor, Database, DatabaseOptions, Error as MdbxError, Mode, NoWriteMap, RO, RW,
    ReadWriteOptions, SyncMode, Table, TableFlags, Transaction, TransactionKind, WriteFlags,
};
use std::{borrow::Cow, collections::BTreeMap, fs, path::Path, sync::Arc, time::Instant};
use tracing::{error, info, warn};

type RawEntry = (Vec<u8>, Vec<u8>);

struct BorrowedRawOverlay<'a>(Vec<(&'a [u8], Option<&'a [u8]>)>);

impl RawOverlaySource for BorrowedRawOverlay<'_> {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        for (key, value) in &self.0 {
            sink.visit(key, *value);
        }
    }
}

const MAINTENANCE_TABLE: &str = "neo_node_metadata";
const ENVIRONMENT_ID_KEY: &[u8] = b"\0neo.storage.environment-id.v1";
const PREFIX_OCCUPANCY_PATH_ENV: &str = "NEO_MDBX_PREFIX_INDEX_PATH";
const MAX_TABLES: u64 = 8;
const CURSOR_WRITE_EXACT_PREFIX: u64 = 64;
const CURSOR_WRITE_SAMPLE_INTERVAL: u64 = 256;

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
    environment_id: Option<[u8; 16]>,
    prefix_occupancy: Option<Arc<PrefixOccupancyIndex>>,
}

impl std::fmt::Debug for MdbxStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdbxStore")
            .field("data_table", &self.data_table)
            .field("read_only", &self.read_only)
            .field("prefix_occupancy", &self.prefix_occupancy)
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

        let environment_id = initialize_environment(&db, read_only)?;
        let prefix_occupancy = load_prefix_occupancy(None, environment_id);

        Ok(Self {
            db: Arc::new(db),
            data_table: DataTable::Canonical,
            read_only,
            environment_id,
            prefix_occupancy,
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

        ensure_named_table(&self.db, name, self.read_only)?;
        let prefix_occupancy = load_prefix_occupancy(Some(name), self.environment_id);

        Ok(Self {
            db: Arc::clone(&self.db),
            data_table: DataTable::Named(Arc::from(name)),
            read_only: self.read_only,
            environment_id: self.environment_id,
            prefix_occupancy,
        })
    }

    /// Returns the logical data-table name, or `None` for the canonical table.
    pub fn data_table_name(&self) -> Option<&str> {
        self.data_table.name()
    }

    pub(super) fn prefix_occupancy(&self) -> Option<Arc<PrefixOccupancyIndex>> {
        self.prefix_occupancy.clone()
    }

    #[cfg(test)]
    pub(super) fn install_prefix_occupancy_for_test(
        &mut self,
        spec: PrefixOccupancySpec,
        keys: &[Vec<u8>],
    ) -> StorageResult<()> {
        let environment_id = self.environment_id.ok_or_else(|| {
            StorageError::invalid_operation("test MDBX environment has no identity")
        })?;
        let transaction_id = self.read_txn()?.id();
        let index = PrefixOccupancyIndex::from_keys(environment_id, transaction_id, spec, keys)?;
        self.prefix_occupancy = Some(Arc::new(index));
        Ok(())
    }

    /// Builds a transaction-bound prefix occupancy artifact by streaming keys
    /// from this store view without reading values.
    pub fn build_prefix_occupancy_index(
        &self,
        output: &Path,
        key_prefix: &[u8],
        key_length: usize,
        prefix_bits: u8,
    ) -> StorageResult<PrefixOccupancyBuildReport> {
        let environment_id = self.environment_id.ok_or_else(|| {
            StorageError::invalid_operation(
                "prefix occupancy build requires a writable-opened environment identity",
            )
        })?;
        let transaction = self.read_txn()?;
        let snapshot_transaction_id = transaction.id();
        let table = transaction
            .open_table(self.data_table.name())
            .map_err(mdbx_error)?;
        let spec = PrefixOccupancySpec::new(
            self.data_table.name().map(str::to_owned),
            key_prefix.to_vec(),
            key_length,
            prefix_bits,
        )?;
        let mut builder =
            PrefixOccupancyBuilder::new(environment_id, snapshot_transaction_id, spec)?;
        let mut cursor = transaction.cursor(&table).map_err(mdbx_error)?;
        let mut entry = cursor
            .set_range::<Cow<'_, [u8]>, ()>(key_prefix)
            .map_err(mdbx_error)?;
        while let Some((key, ())) = entry {
            if !key.starts_with(key_prefix) {
                break;
            }
            builder.insert(&key);
            entry = cursor.next::<Cow<'_, [u8]>, ()>().map_err(mdbx_error)?;
        }
        builder.write(output)
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

    fn read_entries<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(self.data_table.name()).map_err(mdbx_error)?;
        Self::read_entries_with_cursor(&tx, &table, keys)
    }

    pub(super) fn read_entries_with_cursor<K>(
        tx: &Transaction<'_, RO, NoWriteMap>,
        table: &Table<'_>,
        keys: &[K],
    ) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut cursor = tx.cursor(table).map_err(mdbx_error)?;
        keys.iter()
            .map(|key| {
                let key = key.as_ref();
                cursor.set::<Vec<u8>>(key).map_err(mdbx_error)
            })
            .collect()
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
            db.begin_ro_txn().map_err(mdbx_error)
        };
        unsafe {
            // Balance the temporary Arc clone created for the widened borrow.
            drop(Arc::from_raw(db_ptr));
        }
        tx
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
        let mut recorder = MdbxCommitRecorder::start();
        let sort_started = Instant::now();
        let mut entries = overlay.into_iter().collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(key, _)| *key);
        recorder.record_stage(MdbxCommitStage::OverlaySort, elapsed_us(sort_started));
        if entries.is_empty() {
            recorder.finish_success();
            return Ok(());
        }

        let tx = timed_result(&mut recorder, MdbxCommitStage::TransactionOpen, || {
            self.db.begin_rw_txn().map_err(mdbx_error)
        })?;
        let table = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
            tx.create_table(self.data_table.name(), TableFlags::empty())
                .map_err(mdbx_error)
        })?;
        let transaction_id = tx.id();
        recorder.add_count(MdbxCommitCountKind::Tables, 1);
        let mut source = BorrowedRawOverlay(entries);
        let has_entries = apply_overlay(
            &tx,
            &table,
            &mut source,
            &mut recorder,
            self.prefix_occupancy.as_deref(),
        )?;

        if has_entries {
            timed_result(&mut recorder, MdbxCommitStage::Commit, || {
                tx.commit().map_err(mdbx_commit_error)
            })?;
            recorder.mark_committed();
            if let Some(index) = self.prefix_occupancy.as_deref() {
                index.advance_covered_transaction(transaction_id);
            }
        }
        recorder.finish_success();
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

        let mut recorder = MdbxCommitRecorder::start();
        let tx = timed_result(&mut recorder, MdbxCommitStage::TransactionOpen, || {
            self.db.begin_rw_txn().map_err(mdbx_error)
        })?;
        let primary_table = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
            tx.create_table(self.data_table.name(), TableFlags::empty())
                .map_err(mdbx_error)
        })?;
        let transaction_id = tx.id();
        recorder.add_count(MdbxCommitCountKind::Tables, 1);
        apply_overlay(
            &tx,
            &primary_table,
            primary,
            &mut recorder,
            self.prefix_occupancy.as_deref(),
        )?;

        let secondary_table = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
            tx.create_table(secondary_store.data_table.name(), TableFlags::empty())
                .map_err(mdbx_error)
        })?;
        recorder.add_count(MdbxCommitCountKind::Tables, 1);
        apply_overlay(
            &tx,
            &secondary_table,
            secondary,
            &mut recorder,
            secondary_store.prefix_occupancy.as_deref(),
        )?;
        timed_result(&mut recorder, MdbxCommitStage::Commit, || {
            tx.commit().map_err(mdbx_commit_error)
        })?;
        recorder.mark_committed();
        if let Some(index) = self.prefix_occupancy.as_deref() {
            index.advance_covered_transaction(transaction_id);
        }
        if let Some(index) = secondary_store.prefix_occupancy.as_deref() {
            index.advance_covered_transaction(transaction_id);
        }
        recorder.finish_success();
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
        let mut recorder = MdbxCommitRecorder::start();
        let tx = timed_result(&mut recorder, MdbxCommitStage::TransactionOpen, || {
            self.db.begin_rw_txn().map_err(mdbx_error)
        })?;
        let table = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
            tx.create_table(self.data_table.name(), TableFlags::empty())
                .map_err(mdbx_error)
        })?;
        let transaction_id = tx.id();
        recorder.add_count(MdbxCommitCountKind::Tables, 1);
        let has_entries = apply_overlay(
            &tx,
            &table,
            overlay,
            &mut recorder,
            self.prefix_occupancy.as_deref(),
        )?;

        if has_entries {
            timed_result(&mut recorder, MdbxCommitStage::Commit, || {
                tx.commit().map_err(mdbx_commit_error)
            })?;
            recorder.mark_committed();
            if let Some(index) = self.prefix_occupancy.as_deref() {
                index.advance_covered_transaction(transaction_id);
            }
        }
        recorder.finish_success();
        Ok(())
    }
}

fn initialize_environment(
    db: &Database<NoWriteMap>,
    read_only: bool,
) -> StorageResult<Option<[u8; 16]>> {
    if let Some(environment_id) = read_existing_environment_id(db)? {
        return Ok(Some(environment_id));
    }
    if read_only {
        return Ok(None);
    }

    let transaction = db.begin_rw_txn().map_err(mdbx_error)?;
    transaction
        .create_table(None, TableFlags::empty())
        .map_err(mdbx_error)?;
    let metadata = transaction
        .create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
        .map_err(mdbx_error)?;
    let environment_id = match transaction
        .get::<Vec<u8>>(&metadata, ENVIRONMENT_ID_KEY)
        .map_err(mdbx_error)?
    {
        Some(bytes) => parse_environment_id(&bytes)?,
        None => {
            let environment_id = rand::random::<[u8; 16]>();
            transaction
                .put(
                    &metadata,
                    ENVIRONMENT_ID_KEY,
                    environment_id,
                    WriteFlags::UPSERT,
                )
                .map_err(mdbx_error)?;
            environment_id
        }
    };
    transaction.commit().map_err(mdbx_commit_error)?;
    Ok(Some(environment_id))
}

fn read_existing_environment_id(db: &Database<NoWriteMap>) -> StorageResult<Option<[u8; 16]>> {
    let transaction = db.begin_ro_txn().map_err(mdbx_error)?;
    if let Err(error) = transaction.open_table(None) {
        return match error {
            MdbxError::NotFound => Ok(None),
            error => Err(mdbx_error(error)),
        };
    }
    let metadata = match transaction.open_table(Some(MAINTENANCE_TABLE)) {
        Ok(metadata) => metadata,
        Err(MdbxError::NotFound) => return Ok(None),
        Err(error) => return Err(mdbx_error(error)),
    };
    transaction
        .get::<Vec<u8>>(&metadata, ENVIRONMENT_ID_KEY)
        .map_err(mdbx_error)?
        .map(|bytes| parse_environment_id(&bytes))
        .transpose()
}

fn parse_environment_id(bytes: &[u8]) -> StorageResult<[u8; 16]> {
    bytes.try_into().map_err(|_| {
        StorageError::backend("MDBX environment identity has an invalid persisted length")
    })
}

fn ensure_named_table(db: &Database<NoWriteMap>, name: &str, read_only: bool) -> StorageResult<()> {
    let exists = {
        let transaction = db.begin_ro_txn().map_err(mdbx_error)?;
        match transaction.open_table(Some(name)) {
            Ok(_) => true,
            Err(MdbxError::NotFound) => false,
            Err(error) => return Err(mdbx_error(error)),
        }
    };
    if exists {
        return Ok(());
    }
    if read_only {
        return Err(mdbx_error(MdbxError::NotFound));
    }
    let transaction = db.begin_rw_txn().map_err(mdbx_error)?;
    transaction
        .create_table(Some(name), TableFlags::empty())
        .map_err(mdbx_error)?;
    transaction.commit().map_err(mdbx_commit_error)?;
    Ok(())
}

fn load_prefix_occupancy(
    table_name: Option<&str>,
    environment_id: Option<[u8; 16]>,
) -> Option<Arc<PrefixOccupancyIndex>> {
    let path = std::env::var_os(PREFIX_OCCUPANCY_PATH_ENV).map(std::path::PathBuf::from)?;
    let environment_id = environment_id?;
    match PrefixOccupancyIndex::load(&path, table_name, environment_id) {
        Ok(Some(index)) => {
            info!(
                target: "neo",
                path = %path.display(),
                table = table_name.unwrap_or("<canonical>"),
                "loaded MDBX prefix occupancy index"
            );
            Some(Arc::new(index))
        }
        Ok(None) => None,
        Err(error) => {
            warn!(
                target: "neo",
                path = %path.display(),
                table = table_name.unwrap_or("<canonical>"),
                error = %error,
                "MDBX prefix occupancy index is unavailable; authoritative reads remain enabled"
            );
            None
        }
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
        match self.try_get_result(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn try_get_result(&self, key: &Vec<u8>) -> StorageResult<Option<Vec<u8>>> {
        self.read_entry(key)
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
        match self.try_get_bytes_result(key) {
            Ok(value) => value,
            Err(err) => {
                warn!(target: "neo", error = %err, "MDBX get failed");
                None
            }
        }
    }

    fn try_get_bytes_result(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.read_entry(key)
    }

    fn try_get_many_bytes<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.read_entries(keys)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for MdbxStore {
    type FindIterator<'a> = std::vec::IntoIter<(StorageKey, StorageItem)>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self.try_get_result(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX typed get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn try_get_result(&self, key: &StorageKey) -> StorageResult<Option<StorageItem>> {
        Ok(self.read_entry(&key.to_array())?.map(StorageItem::from_bytes))
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
        let transaction_id = tx.id();
        let data = tx
            .create_table(self.data_table.name(), TableFlags::empty())
            .map_err(mdbx_error)?;
        let metadata = tx
            .create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
            .map_err(mdbx_error)?;
        for (key, value) in batch.data_operations() {
            match value {
                Some(value) => {
                    if let Some(index) = self.prefix_occupancy.as_deref() {
                        index.observe_put(key);
                    }
                    tx.put(&data, key, value, WriteFlags::UPSERT)
                        .map_err(mdbx_error)?;
                }
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
        if let Some(index) = self.prefix_occupancy.as_deref() {
            index.advance_covered_transaction(transaction_id);
        }
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
            environment_id: self.environment_id,
            prefix_occupancy: self.prefix_occupancy.clone(),
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
    recorder: &mut MdbxCommitRecorder,
    prefix_occupancy: Option<&PrefixOccupancyIndex>,
) -> StorageResult<bool>
where
    O: RawOverlaySource + ?Sized,
{
    let mut cursor = timed_result(recorder, MdbxCommitStage::CursorOpen, || {
        tx.cursor(table).map_err(mdbx_error)
    })?;
    let mut has_entries = false;
    let mut apply_result = Ok(());
    let mut entries = 0u64;
    let mut puts = 0u64;
    let mut deletes = 0u64;
    let mut key_bytes = 0u64;
    let mut value_bytes = 0u64;
    let mut cursor_write_exact_ns = 0u128;
    let mut cursor_write_weighted_ns = 0u128;
    let mut cursor_write_weighted_entries = 0u64;
    let mut pending_cursor_write_sample_ns = None;
    let mut next_sampled_entry = CURSOR_WRITE_EXACT_PREFIX + 1;
    let visit_started = Instant::now();
    {
        let mut sink = |key: &[u8], value: Option<&[u8]>| {
            if apply_result.is_err() {
                return;
            }
            has_entries = true;
            entries = entries.saturating_add(1);
            key_bytes = key_bytes.saturating_add(key.len() as u64);
            match value {
                Some(value) => {
                    puts = puts.saturating_add(1);
                    value_bytes = value_bytes.saturating_add(value.len() as u64);
                    if let Some(index) = prefix_occupancy {
                        index.observe_put(key);
                    }
                }
                None => deletes = deletes.saturating_add(1),
            }
            let exact_measurement = entries <= CURSOR_WRITE_EXACT_PREFIX;
            let sampled_measurement = entries == next_sampled_entry;
            if sampled_measurement {
                next_sampled_entry =
                    next_sampled_entry.saturating_add(CURSOR_WRITE_SAMPLE_INTERVAL);
            }
            let write_started = (exact_measurement || sampled_measurement).then(Instant::now);
            apply_result = apply_overlay_entry(tx, table, &mut cursor, key, value);
            if let Some(write_started) = write_started {
                let elapsed_ns = write_started.elapsed().as_nanos();
                if exact_measurement {
                    cursor_write_exact_ns = cursor_write_exact_ns.saturating_add(elapsed_ns);
                } else {
                    if let Some(previous_sample_ns) =
                        pending_cursor_write_sample_ns.replace(elapsed_ns)
                    {
                        cursor_write_weighted_ns = cursor_write_weighted_ns.saturating_add(
                            previous_sample_ns
                                .saturating_mul(u128::from(CURSOR_WRITE_SAMPLE_INTERVAL)),
                        );
                        cursor_write_weighted_entries = cursor_write_weighted_entries
                            .saturating_add(CURSOR_WRITE_SAMPLE_INTERVAL);
                    }
                }
            }
        };
        overlay.visit_raw_overlay(&mut sink);
    }
    let overlay_visit_us = elapsed_us(visit_started);
    let cursor_write_us = estimate_cursor_write_us(
        entries,
        cursor_write_exact_ns,
        cursor_write_weighted_ns,
        cursor_write_weighted_entries,
        pending_cursor_write_sample_ns,
    )
    .min(overlay_visit_us);
    recorder.record_stage(MdbxCommitStage::OverlayVisit, overlay_visit_us);
    recorder.record_stage(MdbxCommitStage::CursorWrite, cursor_write_us);
    recorder.add_count(MdbxCommitCountKind::Entries, entries);
    recorder.add_count(MdbxCommitCountKind::Puts, puts);
    recorder.add_count(MdbxCommitCountKind::Deletes, deletes);
    recorder.add_count(MdbxCommitCountKind::KeyBytes, key_bytes);
    recorder.add_count(MdbxCommitCountKind::ValueBytes, value_bytes);
    apply_result?;
    Ok(has_entries)
}

pub(super) fn estimate_cursor_write_us(
    entries: u64,
    exact_ns: u128,
    weighted_ns: u128,
    weighted_entries: u64,
    pending_sample_ns: Option<u128>,
) -> u64 {
    let sampled_entries = entries.saturating_sub(CURSOR_WRITE_EXACT_PREFIX);
    let pending_entries = sampled_entries.saturating_sub(weighted_entries);
    let estimated_sampled_ns = weighted_ns.saturating_add(
        pending_sample_ns
            .unwrap_or_default()
            .saturating_mul(u128::from(pending_entries)),
    );
    exact_ns
        .saturating_add(estimated_sampled_ns)
        .checked_div(1_000)
        .unwrap_or_default()
        .min(u128::from(u64::MAX)) as u64
}

fn timed_result<T, E>(
    recorder: &mut MdbxCommitRecorder,
    stage: MdbxCommitStage,
    action: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let started = Instant::now();
    let result = action();
    recorder.record_stage(stage, elapsed_us(started));
    result
}

fn apply_overlay_entry(
    tx: &Transaction<'_, RW, NoWriteMap>,
    table: &Table<'_>,
    cursor: &mut Cursor<'_, RW>,
    key: &[u8],
    value: Option<&[u8]>,
) -> StorageResult<()> {
    match value {
        Some(value) => cursor
            .put(key, value, WriteFlags::UPSERT)
            .map_err(mdbx_error),
        None => tx.del(table, key, None).map(|_| ()).map_err(mdbx_error),
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

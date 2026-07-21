#![allow(unsafe_code)]

use super::metrics::{
    MdbxCommitCountKind, MdbxCommitRecorder, MdbxCommitStage, VALUE_SIZE_COUNT_KINDS, elapsed_us,
    record_shadow_commit_failure, record_shadow_marker_committed, value_size_bucket_index,
};
use super::prefix_occupancy::{
    PrefixOccupancyBuildReport, PrefixOccupancyBuilder, PrefixOccupancyIndex, PrefixOccupancySpec,
};
use super::rebase::MDBX_REBASE_INCOMPLETE_FILE;
use super::snapshot::MdbxSnapshot;
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{
        CoordinatedCommitMarker, MdbxEnvironmentInfo, RawOverlayCursor, RawOverlaySink,
        RawOverlaySource, ShadowCommitHook, ShadowCommitOutcome, ShadowOverlayEntries, Store,
        StoreBackendKind,
    },
    store_maintenance::StoreMaintenanceBatch,
    transactional_store::{CoordinatedTransactionalStore, TransactionalStore},
    write_store::WriteStore,
};
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
#[cfg(not(feature = "mdbx-write-map"))]
use libmdbx::NoWriteMap;
#[cfg(feature = "mdbx-write-map")]
use libmdbx::WriteMap;
use libmdbx::{
    Cursor, Database, DatabaseOptions, Error as MdbxError, Mode, RO, RW, ReadWriteOptions,
    SyncMode, Table, TableFlags, Transaction, TransactionKind, WriteFlags,
};
use parking_lot::Mutex;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    sync::{Arc, OnceLock, Weak},
    time::Instant,
};
use tracing::{error, info, warn};

type RawEntry = (Vec<u8>, Vec<u8>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorWriteMode {
    /// Perform an independent MDBX tree lookup for each overlay entry.
    Search,
    /// Walk the table cursor in key order and use `CURRENT` for exact rows.
    Merge,
}

#[cfg(feature = "mdbx-write-map")]
pub(super) type MdbxDatabaseKind = WriteMap;
#[cfg(not(feature = "mdbx-write-map"))]
pub(super) type MdbxDatabaseKind = NoWriteMap;

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

pub(super) const MAINTENANCE_TABLE: &str = "neo_node_metadata";
pub(super) const ENVIRONMENT_ID_KEY: &[u8] = b"\0neo.storage.environment-id.v1";
const PREFIX_OCCUPANCY_PATH_ENV: &str = "NEO_MDBX_PREFIX_INDEX_PATH";
const CURSOR_WRITE_MODE_ENV: &str = "NEO_MDBX_CURSOR_WRITE_MODE";
const COALESCE_ENV: &str = "NEO_MDBX_COALESCE";
const NO_MEMINIT_ENV: &str = "NEO_MDBX_NO_MEMINIT";
const MAX_TABLES: u64 = 8;
const CURSOR_WRITE_EXACT_PREFIX: u64 = 64;
const CURSOR_WRITE_SAMPLE_INTERVAL: u64 = 256;
const MAX_MERGE_FORWARD_STEPS_PER_KEY: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum DataTable {
    #[default]
    Canonical,
    Named(Arc<str>),
}

#[derive(Default)]
struct PrefixOccupancyRegistry {
    indexes: Mutex<Vec<Weak<PrefixOccupancyIndex>>>,
}

impl PrefixOccupancyRegistry {
    fn register(&self, index: &Arc<PrefixOccupancyIndex>) {
        let mut indexes = self.indexes.lock();
        indexes.retain(|index| index.strong_count() > 0);
        if !indexes
            .iter()
            .any(|existing| existing.ptr_eq(&Arc::downgrade(index)))
        {
            indexes.push(Arc::downgrade(index));
        }
    }

    fn advance_transaction(&self, transaction_id: u64) {
        let mut indexes = self.indexes.lock();
        indexes.retain(|index| {
            if let Some(index) = index.upgrade() {
                index.advance_covered_transaction(transaction_id);
                true
            } else {
                false
            }
        });
    }

    fn find(&self, table_name: Option<&str>) -> Option<Arc<PrefixOccupancyIndex>> {
        let indexes = self.indexes.lock();
        indexes
            .iter()
            .filter_map(Weak::upgrade)
            .find(|index| index.table_name() == table_name)
    }
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
    db: Arc<Database<MdbxDatabaseKind>>,
    data_table: DataTable,
    read_only: bool,
    environment_id: Option<[u8; 16]>,
    prefix_occupancy: Option<Arc<PrefixOccupancyIndex>>,
    prefix_registry: Arc<PrefixOccupancyRegistry>,
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
        if path.join(MDBX_REBASE_INCOMPLETE_FILE).exists() {
            return Err(StorageError::invalid_operation(format!(
                "MDBX environment {} is an incomplete rebase and cannot be opened",
                path.display()
            )));
        }
        if !read_only {
            fs::create_dir_all(path).map_err(|err| StorageError::Io {
                message: format!(
                    "failed to create MDBX data directory {}: {err}",
                    path.display()
                ),
            })?;
        }

        let db = Database::<MdbxDatabaseKind>::open_with_options(
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
                        sync_mode: configured_sync_mode(),
                        max_size: Some(max_size),
                        growth_step: Some(growth_step),
                        ..Default::default()
                    })
                },
                // Environment-level flags are opt-in experiments. Keep the
                // reference MDBX configuration unchanged unless an operator
                // explicitly requests an A/B run.
                coalesce: configured_flag(COALESCE_ENV),
                no_meminit: configured_flag(NO_MEMINIT_ENV),
                ..Default::default()
            },
        )
        .map_err(|err| StorageError::Io {
            message: format!("failed to open MDBX store at {}: {err}", path.display()),
        })?;

        let environment_id = initialize_environment(&db, read_only)?;
        let prefix_registry = environment_id
            .map(shared_prefix_registry)
            .unwrap_or_else(|| Arc::new(PrefixOccupancyRegistry::default()));
        let prefix_occupancy = load_prefix_occupancy(None, environment_id);
        if let Some(index) = prefix_occupancy.as_ref() {
            prefix_registry.register(index);
        }

        Ok(Self {
            db: Arc::new(db),
            data_table: DataTable::Canonical,
            read_only,
            environment_id,
            prefix_occupancy,
            prefix_registry,
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
        let prefix_occupancy = load_prefix_occupancy(Some(name), self.environment_id)
            .or_else(|| self.prefix_registry.find(Some(name)));
        if let Some(index) = prefix_occupancy.as_ref() {
            self.prefix_registry.register(index);
        }

        Ok(Self {
            db: Arc::clone(&self.db),
            data_table: DataTable::Named(Arc::from(name)),
            read_only: self.read_only,
            environment_id: self.environment_id,
            prefix_occupancy,
            prefix_registry: Arc::clone(&self.prefix_registry),
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
        if let Some(index) = self.prefix_occupancy.as_ref() {
            self.prefix_registry.register(index);
        }
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
                "prefix occupancy build requires a persisted environment identity",
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

    /// Visits raw keys in one prefix with a single forward read cursor.
    ///
    /// Unlike [`ReadOnlyStoreGeneric::find`], this API never materializes the
    /// prefix domain. It is intended for bounded scrubs, reservoir sampling,
    /// and migration tooling whose memory must remain independent of table
    /// cardinality. `maximum=None` walks the complete prefix; `Some(0)` reads
    /// nothing.
    pub fn visit_raw_keys_with_prefix<F>(
        &self,
        key_prefix: &[u8],
        maximum: Option<u64>,
        mut visitor: F,
    ) -> StorageResult<u64>
    where
        F: FnMut(&[u8]),
    {
        if maximum == Some(0) {
            return Ok(0);
        }
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(self.data_table.name()).map_err(mdbx_error)?;
        let mut cursor = tx.cursor(&table).map_err(mdbx_error)?;
        let mut entry = if key_prefix.is_empty() {
            cursor.first::<Cow<'_, [u8]>, ()>().map_err(mdbx_error)?
        } else {
            cursor
                .set_range::<Cow<'_, [u8]>, ()>(key_prefix)
                .map_err(mdbx_error)?
        };
        let mut visited = 0u64;
        while let Some((key, ())) = entry {
            if !key.starts_with(key_prefix) {
                break;
            }
            visitor(key.as_ref());
            visited = visited.saturating_add(1);
            if maximum.is_some_and(|maximum| visited >= maximum) {
                break;
            }
            entry = cursor.next::<Cow<'_, [u8]>, ()>().map_err(mdbx_error)?;
        }
        Ok(visited)
    }

    /// Visits raw key/value rows in one prefix with a single frozen read
    /// transaction and forward cursor.
    ///
    /// This is the migration/scrub counterpart to
    /// [`Self::visit_raw_keys_with_prefix`]. Neither keys nor values outlive
    /// the callback, so memory remains independent of namespace cardinality.
    /// Callback failure stops before the next row and is propagated exactly.
    pub fn visit_raw_entries_with_prefix<F>(
        &self,
        key_prefix: &[u8],
        maximum: Option<u64>,
        mut visitor: F,
    ) -> StorageResult<u64>
    where
        F: FnMut(&[u8], &[u8]) -> StorageResult<()>,
    {
        if maximum == Some(0) {
            return Ok(0);
        }
        let tx = self.db.begin_ro_txn().map_err(mdbx_error)?;
        let table = tx.open_table(self.data_table.name()).map_err(mdbx_error)?;
        let mut cursor = tx.cursor(&table).map_err(mdbx_error)?;
        let mut entry = if key_prefix.is_empty() {
            cursor
                .first::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
                .map_err(mdbx_error)?
        } else {
            cursor
                .set_range::<Cow<'_, [u8]>, Cow<'_, [u8]>>(key_prefix)
                .map_err(mdbx_error)?
        };
        let mut visited = 0u64;
        while let Some((key, value)) = entry {
            if !key.starts_with(key_prefix) {
                break;
            }
            visitor(key.as_ref(), value.as_ref())?;
            visited = visited.saturating_add(1);
            if maximum.is_some_and(|maximum| visited >= maximum) {
                break;
            }
            entry = cursor
                .next::<Cow<'_, [u8]>, Cow<'_, [u8]>>()
                .map_err(mdbx_error)?;
        }
        Ok(visited)
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
        tx: &Transaction<'_, RO, MdbxDatabaseKind>,
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

    /// Reads non-decreasing keys with one forward cursor walk.
    ///
    /// MPT finalization emits unique keys sorted by their content-addressed
    /// hash. Seeking the cursor for every key repeats the same tree descent;
    /// `set_range` plus `next` keeps the cursor monotonic while preserving a
    /// `None` result for keys that fall between persisted entries.
    pub(super) fn read_entries_sorted_with_cursor<K>(
        tx: &Transaction<'_, RO, MdbxDatabaseKind>,
        table: &Table<'_>,
        keys: &[K],
    ) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        if keys
            .windows(2)
            .any(|pair| pair[0].as_ref() > pair[1].as_ref())
        {
            // Keep the method total for callers that violate the documented
            // ordering precondition; never return a result for the wrong key.
            return Self::read_entries_with_cursor(tx, table, keys);
        }

        // A content-addressed miss set can be sorted but still sparse across
        // the table. Bound the speculative forward walk so sparse workloads
        // fall back to independent seeks instead of scanning unrelated rows.
        const MAX_FORWARD_STEPS_PER_KEY: usize = 64;
        let mut cursor = tx.cursor(table).map_err(mdbx_error)?;
        let mut current: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut initialized = false;
        let mut results = Vec::with_capacity(keys.len());

        for key in keys {
            let target = key.as_ref();
            if !initialized {
                current = cursor
                    .set_range::<Cow<'_, [u8]>, Vec<u8>>(target)
                    .map_err(mdbx_error)?
                    .map(|(key, value)| (key.into_owned(), value));
                initialized = true;
            } else if current
                .as_ref()
                .is_some_and(|(current_key, _)| current_key.as_slice() < target)
            {
                let mut forward_steps = 0;
                loop {
                    forward_steps += 1;
                    if forward_steps > MAX_FORWARD_STEPS_PER_KEY {
                        // The cursor is too sparse for this batch. Restarting
                        // from the immutable transaction is correct and keeps
                        // the ordered API from regressing cold miss-heavy
                        // finalization workloads.
                        return Self::read_entries_with_cursor(tx, table, keys);
                    }
                    current = cursor
                        .next::<Cow<'_, [u8]>, Vec<u8>>()
                        .map_err(mdbx_error)?
                        .map(|(key, value)| (key.into_owned(), value));
                    match current.as_ref() {
                        Some((current_key, _)) if current_key.as_slice() < target => {}
                        _ => break,
                    }
                }
            }

            results.push(
                current
                    .as_ref()
                    .filter(|(current_key, _)| current_key.as_slice() == target)
                    .map(|(_, value)| value.clone()),
            );
        }

        Ok(results)
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

    pub(crate) fn read_txn(&self) -> StorageResult<Transaction<'static, RO, MdbxDatabaseKind>> {
        let db_ptr = Arc::into_raw(Arc::clone(&self.db));
        let tx = unsafe {
            // SAFETY: the returned snapshot owns `self.clone()`, which owns an
            // `Arc<Database<MdbxDatabaseKind>>`. That Arc keeps the database alive
            // for at least as long as the transaction field is dropped.
            let db: &'static Database<MdbxDatabaseKind> = &*db_ptr;
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
        self.commit_raw_overlay_with_mode(overlay, configured_cursor_write_mode())
    }

    fn commit_raw_overlay_with_mode<'a, I>(
        &self,
        overlay: I,
        cursor_write_mode: CursorWriteMode,
    ) -> StorageResult<()>
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
            cursor_write_mode,
        )?;

        if has_entries {
            timed_result(&mut recorder, MdbxCommitStage::Commit, || {
                tx.commit().map_err(mdbx_commit_error)
            })?;
            recorder.mark_committed();
            self.prefix_registry.advance_transaction(transaction_id);
        }
        recorder.finish_success();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn commit_raw_overlay_merge_for_test<'a, I>(&self, overlay: I) -> StorageResult<()>
    where
        I: IntoIterator<Item = (&'a [u8], Option<&'a [u8]>)>,
    {
        self.commit_raw_overlay_with_mode(overlay, CursorWriteMode::Merge)
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
        self.commit_coordinated_overlays_with_shadow(primary, secondary_store, secondary, None)
    }

    /// Atomically commits overlays belonging to two isolated views, feeding
    /// the secondary overlay's entries to an optional shadow dual-writer.
    ///
    /// The secondary overlay is captured entry-for-entry (both the visited
    /// channel and cursor-resolved entries) while it is applied. After it is
    /// fully applied and before the transaction commits, `shadow` receives
    /// the captured entries; a returned marker row is written into the
    /// maintenance table inside the same transaction (cold-first ordering:
    /// the shadow's frame is durable before the marker can commit). A shadow
    /// error is counted and logged and never fails the canonical commit —
    /// the transaction simply commits without the marker.
    pub fn commit_coordinated_overlays_with_shadow<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
        shadow: Option<&mut ShadowCommitHook<'_>>,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        self.commit_coordinated_overlays_inner(primary, secondary_store, secondary, shadow, None)
    }

    /// Atomically commits two overlays and one mandatory maintenance marker.
    ///
    /// Unlike the shadow hook, every marker error aborts the transaction. This
    /// is the cold-first publication surface for an authoritative secondary
    /// store whose durable bytes were sealed before this call.
    pub fn commit_coordinated_overlays_with_required_marker<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
        marker: &CoordinatedCommitMarker,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        self.commit_coordinated_overlays_inner(
            primary,
            secondary_store,
            secondary,
            None,
            Some(marker),
        )
    }

    fn commit_coordinated_overlays_inner<P, S>(
        &self,
        primary: &mut P,
        secondary_store: &Self,
        secondary: &mut S,
        shadow: Option<&mut ShadowCommitHook<'_>>,
        required_marker: Option<&CoordinatedCommitMarker>,
    ) -> StorageResult<()>
    where
        P: RawOverlaySource + ?Sized,
        S: RawOverlaySource + ?Sized,
    {
        if shadow.is_some() && required_marker.is_some() {
            return Err(StorageError::invalid_operation(
                "coordinated MDBX commit cannot combine shadow and mandatory markers",
            ));
        }
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
        let cursor_write_mode = configured_cursor_write_mode();
        apply_overlay(
            &tx,
            &primary_table,
            primary,
            &mut recorder,
            self.prefix_occupancy.as_deref(),
            cursor_write_mode,
        )?;

        let secondary_table = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
            tx.create_table(secondary_store.data_table.name(), TableFlags::empty())
                .map_err(mdbx_error)
        })?;
        recorder.add_count(MdbxCommitCountKind::Tables, 1);
        let mut captured = ShadowOverlayEntries::new();
        let mut shadow_marker_staged = false;
        if let Some(marker) = required_marker {
            apply_overlay(
                &tx,
                &secondary_table,
                secondary,
                &mut recorder,
                secondary_store.prefix_occupancy.as_deref(),
                cursor_write_mode,
            )?;
            let maintenance = timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
                tx.create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
                    .map_err(mdbx_error)
            })?;
            recorder.add_count(MdbxCommitCountKind::Tables, 1);
            tx.put(&maintenance, &marker.key, &marker.value, WriteFlags::UPSERT)
                .map_err(mdbx_error)?;
        } else if let Some(hook) = shadow {
            let mut tee = TeeOverlaySource {
                inner: secondary,
                captured: &mut captured,
            };
            apply_overlay(
                &tx,
                &secondary_table,
                &mut tee,
                &mut recorder,
                secondary_store.prefix_occupancy.as_deref(),
                cursor_write_mode,
            )?;
            match hook(std::mem::take(&mut captured)) {
                ShadowCommitOutcome::Prepared(marker) => {
                    let maintenance =
                        timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
                            tx.create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
                                .map_err(mdbx_error)
                        })?;
                    tx.put(&maintenance, &marker.key, &marker.value, WriteFlags::UPSERT)
                        .map_err(mdbx_error)?;
                    shadow_marker_staged = true;
                }
                ShadowCommitOutcome::Unchanged => {}
                ShadowCommitOutcome::Degraded { marker, error } => {
                    let maintenance =
                        timed_result(&mut recorder, MdbxCommitStage::TableOpen, || {
                            tx.create_table(Some(MAINTENANCE_TABLE), TableFlags::empty())
                                .map_err(mdbx_error)
                        })?;
                    tx.put(&maintenance, &marker.key, &marker.value, WriteFlags::UPSERT)
                        .map_err(mdbx_error)?;
                    record_shadow_commit_failure();
                    warn!(
                        target: "neo::storage::mdbx",
                        error = %error,
                        "append shadow dual-write failed; canonical commit records a degraded marker and continues"
                    );
                }
            }
        } else {
            apply_overlay(
                &tx,
                &secondary_table,
                secondary,
                &mut recorder,
                secondary_store.prefix_occupancy.as_deref(),
                cursor_write_mode,
            )?;
        }
        timed_result(&mut recorder, MdbxCommitStage::Commit, || {
            tx.commit().map_err(mdbx_commit_error)
        })?;
        if shadow_marker_staged {
            record_shadow_marker_committed();
        }
        recorder.mark_committed();
        self.prefix_registry.advance_transaction(transaction_id);
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
            configured_cursor_write_mode(),
        )?;

        if has_entries {
            timed_result(&mut recorder, MdbxCommitStage::Commit, || {
                tx.commit().map_err(mdbx_commit_error)
            })?;
            recorder.mark_committed();
            self.prefix_registry.advance_transaction(transaction_id);
        }
        recorder.finish_success();
        Ok(())
    }
}

/// Resolve the optional catch-up sync policy without changing the durable
/// production default. MDBX's no-meta and safe-no-sync modes preserve database
/// integrity but may lose the most recent committed transactions after a
/// crash; callers should use them only for replay/bootstrap jobs that can
/// replay from the last steady checkpoint.
fn configured_sync_mode() -> SyncMode {
    let raw = std::env::var("NEO_MDBX_SYNC_MODE").unwrap_or_else(|_| "durable".to_owned());
    let mode = match parse_sync_mode(&raw) {
        Some(mode) => mode,
        None if raw
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_'], "")
            .starts_with("utterly") =>
        {
            warn!(
                target: "neo",
                requested_mode = %raw,
                "ignoring unsafe MDBX utterly-no-sync request; use durable, no-meta-sync, or safe-no-sync"
            );
            SyncMode::Durable
        }
        None => {
            warn!(
                target: "neo",
                requested_mode = %raw,
                "unknown NEO_MDBX_SYNC_MODE; using durable MDBX commits"
            );
            SyncMode::Durable
        }
    };
    if !matches!(mode, SyncMode::Durable) {
        warn!(
            target: "neo",
            mode = ?mode,
            "MDBX non-durable catch-up sync mode enabled; replay from a steady checkpoint after a crash"
        );
    }
    mode
}

fn configured_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|raw| parse_flag(&raw))
}

fn parse_flag(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn parse_sync_mode(raw: &str) -> Option<SyncMode> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', '_'], "");
    match normalized.as_str() {
        "durable" | "default" => Some(SyncMode::Durable),
        "nometa" | "nometasync" => Some(SyncMode::NoMetaSync),
        "safenosync" => Some(SyncMode::SafeNoSync),
        "utterly" | "utterlynosync" => None,
        _ => None,
    }
}

/// Resolve the cursor writer policy. The merge path is intentionally opt-in
/// until a target-host A/B has established both a throughput benefit and exact
/// root/reopen parity. Any unknown value keeps the established search path.
fn configured_cursor_write_mode() -> CursorWriteMode {
    let raw = std::env::var(CURSOR_WRITE_MODE_ENV).unwrap_or_default();
    match parse_cursor_write_mode(&raw) {
        Some(mode) => mode,
        None if !raw.trim().is_empty() => {
            warn!(
                target: "neo",
                requested_mode = %raw,
                "unknown NEO_MDBX_CURSOR_WRITE_MODE; using independent MDBX cursor writes"
            );
            CursorWriteMode::Search
        }
        None => CursorWriteMode::Search,
    }
}

fn parse_cursor_write_mode(raw: &str) -> Option<CursorWriteMode> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', '_'], "");
    match normalized.as_str() {
        "search" | "default" => Some(CursorWriteMode::Search),
        "merge" | "mergecursor" | "cursormerge" => Some(CursorWriteMode::Merge),
        _ => None,
    }
}

#[cfg(test)]
mod sync_mode_tests {
    use super::{
        CursorWriteMode, MdbxOverlayCursor, configured_flag, parse_cursor_write_mode, parse_flag,
        parse_sync_mode, proc_io_counter, proc_stat_faults, process_resource_snapshot,
    };
    use crate::mdbx::MdbxStoreProvider;
    use crate::persistence::storage::StorageConfig;
    use crate::persistence::{RawOverlayCursor, RawReadOnlyStore};
    use libmdbx::{SyncMode, TableFlags};
    use tempfile::tempdir;

    #[test]
    fn parses_supported_sync_modes_without_weakening_durable_default() {
        assert!(matches!(
            parse_sync_mode("durable"),
            Some(SyncMode::Durable)
        ));
        assert!(matches!(
            parse_sync_mode("no-meta-sync"),
            Some(SyncMode::NoMetaSync)
        ));
        assert!(matches!(
            parse_sync_mode(" SAFE_NO_SYNC "),
            Some(SyncMode::SafeNoSync)
        ));
    }

    #[test]
    fn rejects_unsafe_and_unknown_sync_modes() {
        assert!(parse_sync_mode("utterly_no_sync").is_none());
        assert!(parse_sync_mode("not-a-mode").is_none());
    }

    #[test]
    fn environment_flags_are_opt_in() {
        assert!(!configured_flag("NEO_MDBX_TEST_FLAG_THAT_IS_NOT_SET"));
        for value in ["1", "true", "YES", " on "] {
            assert!(parse_flag(value), "{value:?} should enable the flag");
        }
        for value in ["0", "false", "off", "unknown"] {
            assert!(
                !parse_flag(value),
                "{value:?} should leave the flag disabled"
            );
        }
    }

    #[test]
    fn parses_cursor_write_modes_without_changing_the_default() {
        assert_eq!(
            parse_cursor_write_mode("search"),
            Some(CursorWriteMode::Search)
        );
        assert_eq!(
            parse_cursor_write_mode("merge-cursor"),
            Some(CursorWriteMode::Merge)
        );
        assert!(parse_cursor_write_mode("unknown").is_none());
    }

    #[test]
    fn cursor_resolution_counts_absent_and_existing_rows_in_one_transaction() {
        let root = tempdir().expect("temporary MDBX store");
        let store = MdbxStoreProvider::new(StorageConfig {
            path: root.path().join("cursor-resolution-counters"),
            ..Default::default()
        })
        .get_mdbx_store("")
        .expect("open MDBX store");
        let transaction = store.db.begin_rw_txn().expect("open write transaction");
        let table = transaction
            .create_table(store.data_table.name(), TableFlags::empty())
            .expect("open data table");
        let mut cursor = transaction.cursor(&table).expect("open write cursor");
        let mut facade = MdbxOverlayCursor::new(&mut cursor, None);
        let key = b"insert-first-key";

        assert_eq!(
            facade
                .insert_stored_if_absent(key, b"initial")
                .expect("insert absent row"),
            None
        );
        assert_eq!(
            facade
                .insert_stored_if_absent(key, b"ignored")
                .expect("probe existing row"),
            Some(b"initial".to_vec())
        );
        facade
            .write_stored(key, b"updated")
            .expect("replace positioned row");
        assert_eq!(facade.resolve_absent, 1);
        assert_eq!(facade.resolve_present, 1);
        drop(facade);
        drop(cursor);
        transaction
            .commit()
            .expect("commit insert-first transaction");

        assert_eq!(store.try_get_bytes(key), Some(b"updated".to_vec()));
    }

    #[test]
    fn process_resource_parsers_handle_linux_proc_fields() {
        let io = "rchar: 1\nread_bytes: 4096\nwrite_bytes: 8192\n";
        assert_eq!(proc_io_counter(io, "read_bytes"), Some(4096));
        assert_eq!(proc_io_counter(io, "write_bytes"), Some(8192));
        assert_eq!(proc_io_counter(io, "missing"), None);
        assert_eq!(
            proc_stat_faults("123 (neo node) R 1 2 3 4 5 6 7 8 9 10"),
            Some((7, 9))
        );
        assert!(process_resource_snapshot().is_some());
    }
}

fn initialize_environment(
    db: &Database<MdbxDatabaseKind>,
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

fn read_existing_environment_id(
    db: &Database<MdbxDatabaseKind>,
) -> StorageResult<Option<[u8; 16]>> {
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

fn ensure_named_table(
    db: &Database<MdbxDatabaseKind>,
    name: &str,
    read_only: bool,
) -> StorageResult<()> {
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
            let trusted_startup = index.coverage().1 == u64::MAX;
            info!(
                target: "neo",
                path = %path.display(),
                table = table_name.unwrap_or("<canonical>"),
                baseline_transaction_id = index.coverage().0,
                trusted_startup,
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

fn shared_prefix_registry(environment_id: [u8; 16]) -> Arc<PrefixOccupancyRegistry> {
    static REGISTRIES: OnceLock<Mutex<HashMap<[u8; 16], Weak<PrefixOccupancyRegistry>>>> =
        OnceLock::new();
    let registries = REGISTRIES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut registries = registries.lock();
    if let Some(registry) = registries.get(&environment_id).and_then(Weak::upgrade) {
        return registry;
    }
    let registry = Arc::new(PrefixOccupancyRegistry::default());
    registries.insert(environment_id, Arc::downgrade(&registry));
    registry
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
        Ok(self
            .read_entry(&key.to_array())?
            .map(StorageItem::from_bytes))
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

    fn supports_raw_overlay_cursor(&self) -> bool {
        // `apply_overlay` drives `RawOverlaySource::commit_raw_overlay_at_cursor`
        // against the RW cursor for every overlay commit path.
        true
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
        self.prefix_registry.advance_transaction(transaction_id);
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
            prefix_registry: Arc::clone(&self.prefix_registry),
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

#[derive(Clone, Copy, Debug)]
struct ProcessResourceSnapshot {
    read_bytes: u64,
    write_bytes: u64,
    minor_faults: u64,
    major_faults: u64,
}

impl ProcessResourceSnapshot {
    fn delta_since(self, before: Self) -> Self {
        Self {
            read_bytes: self.read_bytes.saturating_sub(before.read_bytes),
            write_bytes: self.write_bytes.saturating_sub(before.write_bytes),
            minor_faults: self.minor_faults.saturating_sub(before.minor_faults),
            major_faults: self.major_faults.saturating_sub(before.major_faults),
        }
    }
}

fn process_resource_snapshot() -> Option<ProcessResourceSnapshot> {
    let io = fs::read_to_string("/proc/self/io").ok()?;
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let (minor_faults, major_faults) = proc_stat_faults(&stat)?;
    Some(ProcessResourceSnapshot {
        read_bytes: proc_io_counter(&io, "read_bytes")?,
        write_bytes: proc_io_counter(&io, "write_bytes")?,
        minor_faults,
        major_faults,
    })
}

fn proc_io_counter(input: &str, name: &str) -> Option<u64> {
    input.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field == name).then(|| value.trim().parse().ok()).flatten()
    })
}

fn proc_stat_faults(input: &str) -> Option<(u64, u64)> {
    let fields = input
        .get(input.rfind(')')? + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    // After the parenthesized process name, state is field 3 (index 0).
    Some((fields.get(7)?.parse().ok()?, fields.get(9)?.parse().ok()?))
}

fn apply_overlay<O>(
    tx: &Transaction<'_, RW, MdbxDatabaseKind>,
    table: &Table<'_>,
    overlay: &mut O,
    recorder: &mut MdbxCommitRecorder,
    prefix_occupancy: Option<&PrefixOccupancyIndex>,
    cursor_write_mode: CursorWriteMode,
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
    let mut value_size_counts = [0u64; VALUE_SIZE_COUNT_KINDS.len()];
    let mut cursor_write_exact_ns = 0u128;
    let mut cursor_write_weighted_ns = 0u128;
    let mut cursor_write_weighted_entries = 0u64;
    let mut pending_cursor_write_sample_ns = None;
    let mut next_sampled_entry = CURSOR_WRITE_EXACT_PREFIX + 1;
    let mut merge_cursor_state = MergeCursorState::default();
    let mut effective_cursor_write_mode = cursor_write_mode;
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
                    let bucket = value_size_bucket_index(value.len());
                    value_size_counts[bucket] = value_size_counts[bucket].saturating_add(1);
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
            apply_result = match effective_cursor_write_mode {
                CursorWriteMode::Search => apply_overlay_entry(tx, table, &mut cursor, key, value),
                CursorWriteMode::Merge => {
                    apply_overlay_entry_merge(&mut cursor, &mut merge_cursor_state, key, value)
                }
            };
            if matches!(effective_cursor_write_mode, CursorWriteMode::Merge)
                && merge_cursor_state.fallback_to_search
            {
                effective_cursor_write_mode = CursorWriteMode::Search;
            }
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
    // The cursor-write estimate extrapolates plain sink-write samples, so it
    // must be computed from the sink entries only — cursor-resolved journal
    // entries (merged into the counts below) are timed as CursorResolve.
    let sink_entries = entries;
    if apply_result.is_ok() {
        // Cursor-resolved entries (deferred full-state MPT reference
        // resolution) read and replace rows through this same write cursor.
        // Existing rows reuse the positioned cursor; absent rows follow the
        // established cursor probe plus upsert path.
        // Any failure aborts the commit below before the transaction
        // publishes, keeping the overlay fail-closed. Resolution time is
        // recorded as its own stage instead of inflating the overlay visit.
        let resources_before = process_resource_snapshot();
        let resolve_started = Instant::now();
        let resolve_result = {
            let mut facade = MdbxOverlayCursor::new(&mut cursor, prefix_occupancy);
            overlay
                .commit_raw_overlay_at_cursor(&mut facade)
                .map(|()| facade)
        };
        let resolve_us = elapsed_us(resolve_started);
        let resources_after = process_resource_snapshot();
        recorder.record_stage(MdbxCommitStage::CursorResolve, resolve_us);
        match resolve_result {
            Ok(facade) => {
                has_entries |= facade.has_entries;
                entries = entries.saturating_add(facade.entries);
                puts = puts.saturating_add(facade.puts);
                key_bytes = key_bytes.saturating_add(facade.key_bytes);
                value_bytes = value_bytes.saturating_add(facade.value_bytes);
                recorder.add_count(
                    MdbxCommitCountKind::CursorResolvePresent,
                    facade.resolve_present,
                );
                recorder.add_count(
                    MdbxCommitCountKind::CursorResolveAbsent,
                    facade.resolve_absent,
                );
                if facade.entries > 0
                    && let (Some(before), Some(after)) = (resources_before, resources_after)
                {
                    let delta = after.delta_since(before);
                    recorder.add_count(
                        MdbxCommitCountKind::CursorResolveReadBytes,
                        delta.read_bytes,
                    );
                    recorder.add_count(
                        MdbxCommitCountKind::CursorResolveWriteBytes,
                        delta.write_bytes,
                    );
                    recorder.add_count(
                        MdbxCommitCountKind::CursorResolveMinorFaults,
                        delta.minor_faults,
                    );
                    recorder.add_count(
                        MdbxCommitCountKind::CursorResolveMajorFaults,
                        delta.major_faults,
                    );
                }
                for (count, facade_count) in
                    value_size_counts.iter_mut().zip(facade.value_size_counts)
                {
                    *count = count.saturating_add(facade_count);
                }
            }
            Err(error) => apply_result = Err(error),
        }
    }
    let cursor_write_us = estimate_cursor_write_us(
        sink_entries,
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
    for (kind, count) in VALUE_SIZE_COUNT_KINDS.into_iter().zip(value_size_counts) {
        recorder.add_count(kind, count);
    }
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
    tx: &Transaction<'_, RW, MdbxDatabaseKind>,
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

/// Write-cursor facade resolving overlay entries against rows already stored
/// in the table during a fused commit.
///
/// `insert_stored_if_absent` records exact present/absent volumes while
/// preserving the established `set_range` plus positioned-write behavior.
/// Existing rows are replaced with `CURRENT`; absent rows use `UPSERT`.
/// Entries arrive in raw byte-key order from
/// [`RawOverlaySource::commit_raw_overlay_at_cursor`].
struct MdbxOverlayCursor<'cursor, 'txn> {
    cursor: &'cursor mut Cursor<'txn, RW>,
    prefix_occupancy: Option<&'cursor PrefixOccupancyIndex>,
    /// Exact key still selected by the latest successful `read_stored`.
    positioned_key: Option<Vec<u8>>,
    has_entries: bool,
    entries: u64,
    puts: u64,
    key_bytes: u64,
    value_bytes: u64,
    value_size_counts: [u64; VALUE_SIZE_COUNT_KINDS.len()],
    resolve_present: u64,
    resolve_absent: u64,
}

impl<'cursor, 'txn> MdbxOverlayCursor<'cursor, 'txn> {
    fn new(
        cursor: &'cursor mut Cursor<'txn, RW>,
        prefix_occupancy: Option<&'cursor PrefixOccupancyIndex>,
    ) -> Self {
        Self {
            cursor,
            prefix_occupancy,
            positioned_key: None,
            has_entries: false,
            entries: 0,
            puts: 0,
            key_bytes: 0,
            value_bytes: 0,
            value_size_counts: [0; VALUE_SIZE_COUNT_KINDS.len()],
            resolve_present: 0,
            resolve_absent: 0,
        }
    }

    fn record_put(&mut self, key: &[u8], value: &[u8]) {
        self.has_entries = true;
        self.entries = self.entries.saturating_add(1);
        self.puts = self.puts.saturating_add(1);
        self.key_bytes = self.key_bytes.saturating_add(key.len() as u64);
        self.value_bytes = self.value_bytes.saturating_add(value.len() as u64);
        let bucket = value_size_bucket_index(value.len());
        self.value_size_counts[bucket] = self.value_size_counts[bucket].saturating_add(1);
        if let Some(index) = self.prefix_occupancy {
            index.observe_put(key);
        }
    }
}

impl RawOverlayCursor for MdbxOverlayCursor<'_, '_> {
    fn read_stored(&mut self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        match self.cursor.set_range::<Vec<u8>, Vec<u8>>(key) {
            Ok(Some((found_key, value))) if found_key.as_slice() == key => {
                self.positioned_key = Some(found_key);
                Ok(Some(value))
            }
            Ok(_) => {
                self.positioned_key = None;
                Ok(None)
            }
            Err(error) => Err(mdbx_error(error)),
        }
    }

    fn write_stored(&mut self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        // When the probe left the cursor on this exact key, `CURRENT`
        // replaces the row without a second descent; otherwise `UPSERT`
        // inserts or replaces it independently of the cursor position.
        let flags = if self.positioned_key.as_deref() == Some(key) {
            WriteFlags::CURRENT
        } else {
            WriteFlags::UPSERT
        };
        self.cursor.put(key, value, flags).map_err(mdbx_error)?;
        self.positioned_key = None;
        self.record_put(key, value);
        Ok(())
    }

    fn insert_stored_if_absent(
        &mut self,
        key: &[u8],
        absent_value: &[u8],
    ) -> StorageResult<Option<Vec<u8>>> {
        match self.read_stored(key)? {
            Some(stored) => {
                self.resolve_present = self.resolve_present.saturating_add(1);
                Ok(Some(stored))
            }
            None => {
                self.write_stored(key, absent_value)?;
                self.resolve_absent = self.resolve_absent.saturating_add(1);
                Ok(None)
            }
        }
    }
}

#[derive(Default)]
struct MergeCursorState {
    /// The key currently selected by the cursor, if the table is not exhausted.
    current_key: Option<Vec<u8>>,
    /// Lower bound from which a failed seek or end-of-table proves no row exists.
    exhausted_from: Option<Vec<u8>>,
    initialized: bool,
    fallback_to_search: bool,
}

/// Applies one ordered overlay entry using a single forward cursor walk.
///
/// Exact rows are already selected when the overlay reaches them, so `CURRENT`
/// avoids another B-tree descent. Inserts still use `UPSERT`, which is the
/// correct MDBX operation for a key absent from the cursor's current position.
/// If a source violates the ordering contract, the helper seeks backward and
/// remains semantically equivalent to the independent-search implementation.
fn apply_overlay_entry_merge(
    cursor: &mut Cursor<'_, RW>,
    state: &mut MergeCursorState,
    key: &[u8],
    value: Option<&[u8]>,
) -> StorageResult<()> {
    let seek_backward = state
        .current_key
        .as_deref()
        .is_some_and(|current| current > key)
        || state.current_key.is_none()
            && state
                .exhausted_from
                .as_deref()
                .is_some_and(|exhausted_from| key < exhausted_from);
    if !state.initialized || seek_backward {
        state.current_key = cursor
            .set_range::<Vec<u8>, ()>(key)
            .map_err(mdbx_error)?
            .map(|(key, _)| key);
        state.initialized = true;
        state.exhausted_from = state.current_key.is_none().then(|| key.to_vec());
    }

    let mut forward_steps = 0usize;
    while state
        .current_key
        .as_deref()
        .is_some_and(|current| current < key)
    {
        forward_steps += 1;
        if forward_steps > MAX_MERGE_FORWARD_STEPS_PER_KEY {
            // Content-addressed overlays are often sparse relative to the
            // backing table. Bound the speculative walk so sparse batches do
            // not scan the entire MDBX tree for every overlay key.
            state.current_key = cursor
                .set_range::<Vec<u8>, ()>(key)
                .map_err(mdbx_error)?
                .map(|(key, _)| key);
            state.exhausted_from = state.current_key.is_none().then(|| key.to_vec());
            state.fallback_to_search = true;
            break;
        }
        let previous_key = state.current_key.take().expect("cursor key checked above");
        state.current_key = cursor
            .next::<Vec<u8>, ()>()
            .map_err(mdbx_error)?
            .map(|(key, _)| key);
        if state.current_key.is_none() {
            state.exhausted_from = Some(previous_key);
        } else {
            state.exhausted_from = None;
        }
    }

    if state.current_key.as_deref() == Some(key) {
        match value {
            Some(value) => cursor
                .put(key, value, WriteFlags::CURRENT)
                .map_err(mdbx_error),
            None => {
                cursor.del(WriteFlags::CURRENT).map_err(mdbx_error)?;
                match cursor.get_current::<Vec<u8>, ()>().map_err(mdbx_error)? {
                    Some((next_key, _)) => {
                        state.current_key = Some(next_key);
                        state.exhausted_from = None;
                    }
                    None => {
                        state.current_key = None;
                        state.exhausted_from = Some(key.to_vec());
                    }
                }
                Ok(())
            }
        }
    } else if let Some(value) = value {
        cursor
            .put(key, value, WriteFlags::UPSERT)
            .map_err(mdbx_error)?;
        state.current_key = Some(key.to_vec());
        state.exhausted_from = None;
        Ok(())
    } else {
        Ok(())
    }
}

/// Captures every entry a secondary overlay writes (visited entries and
/// cursor-resolved entries) while forwarding to the real source. Used only
/// by the shadow dual-write path; capture allocates one clone per entry.
struct TeeOverlaySource<'a, O: RawOverlaySource + ?Sized> {
    inner: &'a mut O,
    captured: &'a mut ShadowOverlayEntries,
}

impl<O> RawOverlaySource for TeeOverlaySource<'_, O>
where
    O: RawOverlaySource + ?Sized,
{
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        let captured = &mut *self.captured;
        self.inner
            .visit_raw_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
                captured.push((key.to_vec(), value.map(<[u8]>::to_vec)));
                sink.visit(key, value);
            });
    }

    fn commit_raw_overlay_at_cursor(
        &mut self,
        cursor: &mut dyn RawOverlayCursor,
    ) -> StorageResult<()> {
        let mut tee_cursor = TeeOverlayCursor {
            inner: cursor,
            captured: self.captured,
        };
        self.inner.commit_raw_overlay_at_cursor(&mut tee_cursor)
    }
}

/// Forwards cursor operations while capturing the final bytes written for
/// each key (the shadow needs the cursor-resolved values, not the probes).
struct TeeOverlayCursor<'a> {
    inner: &'a mut dyn RawOverlayCursor,
    captured: &'a mut ShadowOverlayEntries,
}

impl RawOverlayCursor for TeeOverlayCursor<'_> {
    fn read_stored(&mut self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        self.inner.read_stored(key)
    }

    fn write_stored(&mut self, key: &[u8], value: &[u8]) -> StorageResult<()> {
        self.captured.push((key.to_vec(), Some(value.to_vec())));
        self.inner.write_stored(key, value)
    }

    fn insert_stored_if_absent(
        &mut self,
        key: &[u8],
        absent_value: &[u8],
    ) -> StorageResult<Option<Vec<u8>>> {
        let stored = self.inner.insert_stored_if_absent(key, absent_value)?;
        if stored.is_none() {
            self.captured
                .push((key.to_vec(), Some(absent_value.to_vec())));
        }
        Ok(stored)
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

use super::prefix_occupancy::PrefixOccupancyIndex;
use super::store::{MdbxDatabaseKind, MdbxStore, collect_cursor_entries};
use crate::persistence::{
    read_only_store::RawReadOnlyStore,
    read_only_store::ReadOnlyStoreGeneric,
    seek_direction::SeekDirection,
    store_snapshot::{SnapshotCommitResult, StoreSnapshot},
    write_store::WriteStore,
};
use libmdbx::{RO, Table, Transaction};
use parking_lot::RwLock;
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use tracing::{error, info};

type WriteBatch = Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>;

/// Minimum key count before multi-threaded MDBX batch reads are considered.
///
/// Dense StateService finalization batches often sit in the 4k–12k key range
/// during catch-up; a 16k floor left those hot batches serial on a single cursor.
const PARALLEL_BATCH_READ_MIN_KEYS: usize = 4_096;
const MAX_PARALLEL_BATCH_READERS: usize = 16;

/// Mutable point-in-time snapshot over an MDBX store.
pub struct MdbxSnapshot {
    // The table handle is database-scoped and must drop before the store Arc.
    data_table: Option<Table<'static>>,
    // Keep this field before `store` so the read transaction is dropped before
    // the database Arc that keeps its widened lifetime valid.
    read_tx: Option<Transaction<'static, RO, MdbxDatabaseKind>>,
    initialization_error: Option<crate::StorageError>,
    store: Arc<MdbxStore>,
    prefix_occupancy: Option<Arc<PrefixOccupancyIndex>>,
    write_batch: WriteBatch,
}

impl std::fmt::Debug for MdbxSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdbxSnapshot").finish_non_exhaustive()
    }
}

impl MdbxSnapshot {
    #[allow(unsafe_code)]
    pub(crate) fn new(store: Arc<MdbxStore>) -> Self {
        let (data_table, read_tx, initialization_error) = match store.read_txn() {
            Ok(read_tx) => match read_tx.open_table(store.data_table_name()) {
                Ok(data_table) => {
                    // SAFETY: libmdbx table handles are database-scoped and may
                    // be shared by transactions. `store` retains the database
                    // Arc until after `data_table` is dropped.
                    let data_table =
                        unsafe { std::mem::transmute::<Table<'_>, Table<'static>>(data_table) };
                    (Some(data_table), Some(read_tx), None)
                }
                Err(err) => {
                    let error = crate::StorageError::backend(format!(
                        "MDBX snapshot table open failed: {err}"
                    ));
                    error!(target: "neo", error = %error, "MDBX snapshot table open failed");
                    (None, None, Some(error))
                }
            },
            Err(err) => {
                let error =
                    crate::StorageError::backend(format!("MDBX snapshot open failed: {err}"));
                error!(target: "neo", error = %error, "MDBX snapshot open failed");
                (None, None, Some(error))
            }
        };
        let prefix_occupancy = store.prefix_occupancy();
        Self {
            data_table,
            read_tx,
            initialization_error,
            store,
            prefix_occupancy,
            write_batch: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    #[cfg(test)]
    pub(super) fn with_initialization_error(
        store: Arc<MdbxStore>,
        error: crate::StorageError,
    ) -> Self {
        let prefix_occupancy = store.prefix_occupancy();
        Self {
            data_table: None,
            read_tx: None,
            initialization_error: Some(error),
            store,
            prefix_occupancy,
            write_batch: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    fn read_handles(
        &self,
    ) -> crate::StorageResult<(&Transaction<'static, RO, MdbxDatabaseKind>, &Table<'static>)> {
        if let Some(error) = self.initialization_error.as_ref() {
            return Err(error.clone());
        }
        match (self.read_tx.as_ref(), self.data_table.as_ref()) {
            (Some(read_tx), Some(data_table)) => Ok((read_tx, data_table)),
            _ => Err(crate::StorageError::backend(
                "MDBX snapshot read handles are unavailable",
            )),
        }
    }

    fn read_entry(&self, key: &[u8]) -> crate::StorageResult<Option<Vec<u8>>> {
        let (read_tx, data_table) = self.read_handles()?;
        read_tx
            .get::<Vec<u8>>(data_table, key)
            .map_err(super::store::mdbx_error)
    }

    fn read_entries<K>(&self, keys: &[K]) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.read_entries_with_parallelism(keys, batch_read_parallelism())
    }

    fn read_entries_with_parallelism<K>(
        &self,
        keys: &[K],
        parallelism: usize,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        let (read_tx, data_table) = self.read_handles()?;
        if let Some(index) = self.prefix_occupancy.as_deref() {
            let (baseline_transaction_id, covered_transaction_id) = index.coverage();
            let mut candidate_indices = Vec::new();
            let mut candidate_keys = Vec::new();
            for (position, key) in keys.iter().enumerate() {
                let key = key.as_ref();
                if index.may_contain(read_tx.id(), key) != Some(false) {
                    candidate_indices.push(position);
                    candidate_keys.push(key);
                }
            }
            let definite_absences = keys.len().saturating_sub(candidate_keys.len());
            if definite_absences > 0 {
                info!(
                    target: "neo",
                    snapshot_transaction_id = read_tx.id(),
                    queries = keys.len(),
                    definite_absences,
                    candidates = candidate_keys.len(),
                    "MDBX prefix occupancy filtered batch lookup"
                );
                let values = self.read_entries_authoritative(
                    read_tx,
                    data_table,
                    &candidate_keys,
                    parallelism,
                )?;
                if values.len() != candidate_indices.len() {
                    return Err(crate::StorageError::backend(
                        "MDBX prefix occupancy candidate read omitted an input key",
                    ));
                }
                let mut results = vec![None; keys.len()];
                for (position, value) in candidate_indices.into_iter().zip(values) {
                    results[position] = value;
                }
                return Ok(results);
            }
            info!(
                target: "neo",
                snapshot_transaction_id = read_tx.id(),
                baseline_transaction_id,
                covered_transaction_id,
                queries = keys.len(),
                "MDBX prefix occupancy did not filter this batch"
            );
        }

        self.read_entries_authoritative(read_tx, data_table, keys, parallelism)
    }

    fn read_entries_authoritative<K>(
        &self,
        read_tx: &Transaction<'static, RO, MdbxDatabaseKind>,
        data_table: &Table<'static>,
        keys: &[K],
        parallelism: usize,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        let parallelism = parallelism.min(keys.len());
        if parallelism <= 1 || keys.len() < PARALLEL_BATCH_READ_MIN_KEYS {
            return MdbxStore::read_entries_with_cursor(read_tx, data_table, keys);
        }

        let borrowed_keys = keys.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        self.read_entries_parallel(read_tx.id(), &borrowed_keys, parallelism, false)
            .or_else(|error| {
                error!(target: "neo", error = %error, "parallel MDBX batch read failed; preserving correctness with the frozen serial snapshot");
                MdbxStore::read_entries_with_cursor(read_tx, data_table, keys)
            })
    }

    fn read_entries_sorted_authoritative<K>(
        &self,
        read_tx: &Transaction<'static, RO, MdbxDatabaseKind>,
        data_table: &Table<'static>,
        keys: &[K],
        parallelism: usize,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if parallelism <= 1 || keys.len() < PARALLEL_BATCH_READ_MIN_KEYS {
            return MdbxStore::read_entries_sorted_with_cursor(read_tx, data_table, keys);
        }

        // Sorted batches are still split into contiguous chunks, preserving
        // the caller's result order while allowing independent read cursors to
        // resolve sparse content-addressed misses concurrently. If the MDBX
        // reader pool cannot reproduce the frozen transaction, fall back to
        // the serial ordered cursor before returning an error.
        let borrowed_keys = keys.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        self.read_entries_parallel(read_tx.id(), &borrowed_keys, parallelism, true)
            .or_else(|error| {
                error!(
                    target: "neo",
                    error = %error,
                    "parallel sorted MDBX batch read failed; preserving correctness with the frozen ordered cursor"
                );
                MdbxStore::read_entries_sorted_with_cursor(read_tx, data_table, keys)
            })
    }

    #[cfg(test)]
    pub(super) fn try_get_many_bytes_with_parallelism<K>(
        &self,
        keys: &[K],
        parallelism: usize,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.read_entries_with_parallelism(keys, parallelism)
    }

    #[cfg(test)]
    pub(super) fn try_get_many_bytes_parallel_for_test<K>(
        &self,
        keys: &[K],
        parallelism: usize,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        let (read_tx, _) = self.read_handles()?;
        let borrowed_keys = keys.iter().map(AsRef::as_ref).collect::<Vec<_>>();
        self.read_entries_parallel(read_tx.id(), &borrowed_keys, parallelism, false)
    }

    fn read_entries_parallel(
        &self,
        snapshot_id: u64,
        keys: &[&[u8]],
        parallelism: usize,
        sorted: bool,
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>> {
        let worker_count = parallelism.clamp(1, keys.len());
        let mut transactions = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let transaction = self.store.read_txn()?;
            if transaction.id() != snapshot_id {
                return Err(crate::StorageError::backend(
                    "parallel MDBX readers did not match the frozen snapshot",
                ));
            }
            transactions.push(transaction);
        }

        let chunk_size = keys.len().div_ceil(worker_count);
        std::thread::scope(|scope| {
            let mut workers = Vec::with_capacity(worker_count);
            for (transaction, chunk) in transactions.into_iter().zip(keys.chunks(chunk_size)) {
                let table_name = self.store.data_table_name();
                workers.push(scope.spawn(move || {
                    let table = transaction
                        .open_table(table_name)
                        .map_err(super::store::mdbx_error)?;
                    if sorted {
                        MdbxStore::read_entries_sorted_with_cursor(&transaction, &table, chunk)
                    } else {
                        MdbxStore::read_entries_with_cursor(&transaction, &table, chunk)
                    }
                }));
            }

            let mut values = Vec::with_capacity(keys.len());
            for worker in workers {
                let chunk = worker.join().map_err(|_| {
                    crate::StorageError::backend("parallel MDBX batch reader panicked")
                })??;
                values.extend(chunk);
            }
            Ok(values)
        })
    }

    fn collect_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> crate::StorageResult<Vec<(Vec<u8>, Vec<u8>)>> {
        let (read_tx, data_table) = self.read_handles()?;
        let mut cursor = read_tx
            .cursor(data_table)
            .map_err(super::store::mdbx_error)?;
        collect_cursor_entries(&mut cursor, key_prefix, direction)
    }
}

fn batch_read_parallelism() -> usize {
    static PARALLELISM: OnceLock<usize> = OnceLock::new();
    *PARALLELISM.get_or_init(|| {
        // Default remains serial: multi-run A/B on dense MainNet windows
        // (NEO_MDBX_BATCH_READ_THREADS=4 vs 1) measured a ~5% dense-window
        // regression on this host while preserving official roots. Parallelism
        // stays opt-in for cold/high-height experiments.
        std::env::var("NEO_MDBX_BATCH_READ_THREADS")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, MAX_PARALLEL_BATCH_READERS)
    })
}

fn write_intent_batch_read_parallelism() -> usize {
    static PARALLELISM: OnceLock<usize> = OnceLock::new();
    *PARALLELISM.get_or_init(|| {
        // Write-intent reads are the sparse, content-addressed lookup set used
        // immediately before an MPT overlay commit. Keep this override
        // separate from ordinary/pruning reads, whose parallel A/B regressed.
        std::env::var("NEO_MDBX_WRITE_INTENT_READ_THREADS")
            .ok()
            .or_else(|| std::env::var("NEO_MDBX_BATCH_READ_THREADS").ok())
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, MAX_PARALLEL_BATCH_READERS)
    })
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MdbxSnapshot {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        match self.try_get_result(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX snapshot get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn try_get_result(&self, key: &Vec<u8>) -> crate::StorageResult<Option<Vec<u8>>> {
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
                error!(target: "neo", error = %err, "MDBX snapshot find failed - this may cause incorrect state");
                Vec::new().into_iter()
            }
        }
    }
}

impl RawReadOnlyStore for MdbxSnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.try_get_bytes_result(key) {
            Ok(value) => value,
            Err(err) => {
                error!(target: "neo", error = %err, "MDBX snapshot get failed - this is a critical error that may cause incorrect state");
                None
            }
        }
    }

    fn try_get_bytes_result(&self, key: &[u8]) -> crate::StorageResult<Option<Vec<u8>>> {
        self.read_entry(key)
    }

    fn try_get_many_bytes<K>(&self, keys: &[K]) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.read_entries(keys)
    }

    fn try_get_many_bytes_sorted<K>(&self, keys: &[K]) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let (read_tx, data_table) = self.read_handles()?;
        if let Some(index) = self.prefix_occupancy.as_deref() {
            let mut candidate_indices = Vec::new();
            let mut candidate_keys = Vec::new();
            for (position, key) in keys.iter().enumerate() {
                let key = key.as_ref();
                if index.may_contain(read_tx.id(), key) != Some(false) {
                    candidate_indices.push(position);
                    candidate_keys.push(key);
                }
            }
            if candidate_indices.len() < keys.len() {
                let values = self.read_entries_sorted_authoritative(
                    read_tx,
                    data_table,
                    &candidate_keys,
                    batch_read_parallelism(),
                )?;
                if values.len() != candidate_indices.len() {
                    return Err(crate::StorageError::backend(
                        "MDBX prefix occupancy sorted candidate read omitted an input key",
                    ));
                }
                let mut results = vec![None; keys.len()];
                for (position, value) in candidate_indices.into_iter().zip(values) {
                    results[position] = value;
                }
                return Ok(results);
            }
        }
        self.read_entries_sorted_authoritative(read_tx, data_table, keys, batch_read_parallelism())
    }

    fn try_get_many_bytes_sorted_for_write<K>(
        &self,
        keys: &[K],
    ) -> crate::StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let (read_tx, data_table) = self.read_handles()?;
        // The ordered MPT finalizer writes these keys in the next MDBX
        // transaction. Keep the authoritative misses here so MDBX has
        // traversed the same B-tree pages before the writer arrives. Using
        // the occupancy bitmap would save reads but makes the subsequent
        // cursor writes cold on large full-state batches.
        self.read_entries_sorted_authoritative(
            read_tx,
            data_table,
            keys,
            write_intent_batch_read_parallelism(),
        )
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for MdbxSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> crate::StorageResult<()> {
        self.write_batch.write().insert(key, None);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::StorageResult<()> {
        self.write_batch.write().insert(key, Some(value));
        Ok(())
    }
}

impl StoreSnapshot for MdbxSnapshot {
    type Store = MdbxStore;

    fn store(&self) -> Arc<Self::Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> SnapshotCommitResult {
        if let Some(error) = self.initialization_error.as_ref() {
            return Err(error.clone());
        }
        {
            let batch = self.write_batch.read();
            self.store.commit_overlay(&batch)?;
        }
        self.write_batch.write().clear();
        Ok(())
    }
}

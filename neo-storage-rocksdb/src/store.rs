use crate::write_batch_buffer::{WriteBatchConfig, WriteBatchStatsSnapshot};
use neo_storage::persistence::{
    read_cache::{ReadCacheConfig, StorageReadCache},
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    storage::StorageConfig,
    store::{OnNewSnapshotDelegate, Store},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use neo_storage::{StorageItem, StorageKey, StorageResult};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    DB, DBIteratorWithThreadMode, ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions,
};
use std::{collections::BTreeMap, fs, mem, sync::Arc, time::Instant};
use tracing::{debug, error, warn};

use super::provider::{self, BatchCommitter, ReadAheadConfig};

pub struct RocksDbStore {
    pub(crate) db: Arc<DB>,
    pub(crate) on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
    pub(crate) batch_committer: Arc<BatchCommitter>,
    pub(crate) batch_config: RwLock<WriteBatchConfig>,
    pub(crate) read_cache: Option<Arc<StorageReadCache>>,
    pub(crate) read_ahead_config: ReadAheadConfig,
}

impl RocksDbStore {
    pub(crate) fn open(
        config: &StorageConfig,
        batch_config: WriteBatchConfig,
        read_cache_config: &Option<ReadCacheConfig>,
        enable_bloom_filters: bool,
        enable_read_ahead: bool,
    ) -> Result<Self, rocksdb::Error> {
        if let Some(parent) = config.path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(err) = fs::create_dir_all(parent) {
                    warn!(
                        target: "neo",
                        path = %config.path.display(),
                        error = %err,
                        "failed to create RocksDB data directory"
                    );
                }
            }
        }

        let options = provider::build_db_options(config, enable_bloom_filters);
        let db = if config.read_only {
            Arc::new(DB::open_for_read_only(&options, &config.path, false)?)
        } else {
            Arc::new(DB::open(&options, &config.path)?)
        };

        let batch_committer = Arc::new(BatchCommitter::new(Arc::clone(&db), batch_config));

        let read_cache = read_cache_config
            .as_ref()
            .map(|cfg| Arc::new(StorageReadCache::new(*cfg)));

        let read_ahead_config = ReadAheadConfig {
            enabled: enable_read_ahead,
            ..Default::default()
        };

        Ok(Self {
            db,
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
            batch_committer,
            batch_config: RwLock::new(batch_config),
            read_cache,
            read_ahead_config,
        })
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        provider::iterator_from(
            self.db.as_ref(),
            None,
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        // Check read cache first
        // Note: Vec<u8> doesn't implement StorageKey, so we skip caching for raw bytes
        match self.db.get(key) {
            Ok(value) => value,
            Err(err) => {
                warn!(target: "neo", error = %err, "rocksdb get failed");
                None
            }
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if direction == SeekDirection::Backward {
            if let Some(prefix) = key_prefix {
                let iterator = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    None,
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return Box::new(iterator.filter_map(|res| match res {
                    Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                    Err(err) => {
                        warn!(target: "neo", error = %err, "rocksdb iterator error");
                        None
                    }
                }));
            }
        }

        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        Box::new(iterator.filter_map(|res| res.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        // Check read cache first (bloom filter will be checked inside)
        if let Some(ref cache) = self.read_cache {
            if let Some(item) = cache.get(key) {
                return Some(item);
            }
        }

        let raw = key.to_array();
        let result = self.db.get(raw).ok().flatten().map(StorageItem::from_bytes);

        // Cache the result if found
        if let (Some(cache), Some(item)) = (&self.read_cache, &result) {
            let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
            cache.put(key.clone(), item.clone(), size);
        }

        result
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iter = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    None,
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                let read_cache = self.read_cache.clone();
                return Box::new(iter.filter_map(move |res| {
                    let (key, value) = match res {
                        Ok(entry) => entry,
                        Err(err) => {
                            warn!(target: "neo", error = %err, "rocksdb iterator error");
                            return None;
                        }
                    };
                    let key_vec: Vec<u8> = key.into();
                    let storage_key = StorageKey::from_bytes(&key_vec);
                    let storage_item = StorageItem::from_bytes(value.into());
                    if let Some(ref cache) = read_cache {
                        let size =
                            storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                        cache.put(storage_key.clone(), storage_item.clone(), size);
                    }
                    Some((storage_key, storage_item))
                }));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);

        // Create an iterator that also caches results
        let read_cache = self.read_cache.clone();

        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(target: "neo", error = %err, "rocksdb iterator error");
                    return None;
                }
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes {
                if !key_vec.starts_with(prefix) {
                    return None;
                }
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            let storage_item = StorageItem::from_bytes(value.into());

            // Cache the result
            if let Some(ref cache) = read_cache {
                let size = storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                cache.put(storage_key.clone(), storage_item.clone(), size);
            }

            Some((storage_key, storage_item))
        }))
    }
}

impl ReadOnlyStore for RocksDbStore {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.db.delete(&key).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb delete failed");
            neo_storage::StorageError::Io {
                message: format!("RocksDB delete failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.db.put(&key, &value).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put failed");
            neo_storage::StorageError::Io {
                message: format!("RocksDB put failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> neo_storage::StorageResult<()> {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db.put_opt(&key, &value, &options).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put_sync failed");
            neo_storage::StorageError::Io {
                message: format!("RocksDB put_sync failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }
}

impl Store for RocksDbStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        let store_arc = Arc::new(self.clone());
        let snapshot = Arc::new(RocksDbSnapshot::new(
            self.db.clone(),
            store_arc,
            self.read_cache.clone(),
            self.read_ahead_config,
        ));

        let handlers = self.on_new_snapshot.read();
        for handler in handlers.iter() {
            handler(self, snapshot.clone());
        }

        snapshot
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.on_new_snapshot.write().push(handler);
    }

    fn enable_fast_sync_mode(&self) {
        RocksDbStore::enable_fast_sync_mode(self);
    }

    fn disable_fast_sync_mode(&self) {
        RocksDbStore::disable_fast_sync_mode(self);
    }

    fn flush(&self) {
        if let Err(err) = self.flush_batch_writes() {
            warn!(target: "neo", error = %err, "failed to flush pending RocksDB batch writes");
        }
        self.flush_memtables();
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Clone for RocksDbStore {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
            batch_committer: Arc::clone(&self.batch_committer),
            batch_config: RwLock::new(*self.batch_config.read()),
            read_cache: self.read_cache.clone(),
            read_ahead_config: self.read_ahead_config,
        }
    }
}

impl Drop for RocksDbStore {
    fn drop(&mut self) {
        if let Err(err) = self.flush_batch_writes() {
            warn!(
                target: "neo",
                error = %err,
                "RocksDbStore: failed to flush pending batch writes during drop"
            );
        }
    }
}

pub struct RocksDbSnapshot {
    store: Arc<RocksDbStore>,
    db: Arc<DB>,
    snapshot: DbSnapshot<'static>,
    write_batch: Mutex<WriteBatch>,
    pending_changes: Mutex<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Optional read cache for this snapshot
    read_cache: Option<Arc<StorageReadCache>>,
    /// Read-ahead configuration
    read_ahead_config: ReadAheadConfig,
}

impl RocksDbSnapshot {
    fn new(
        db: Arc<DB>,
        store: Arc<RocksDbStore>,
        read_cache: Option<Arc<StorageReadCache>>,
        read_ahead_config: ReadAheadConfig,
    ) -> Self {
        let snapshot = Self::create_snapshot(&db);

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
            pending_changes: Mutex::new(BTreeMap::new()),
            read_cache,
            read_ahead_config,
        }
    }

    fn create_snapshot(db: &Arc<DB>) -> DbSnapshot<'static> {
        // Create a snapshot using a `'static` DB reference while keeping the Arc alive.
        let db_ptr = Arc::into_raw(db.clone());
        let snapshot = unsafe {
            // SAFETY: `db_ptr` comes from an Arc clone that stays alive for this scope.
            // The `RocksDbSnapshot` struct also owns an `Arc<DB>`, so the DB outlives
            // the snapshot. We immediately balance the raw Arc below.
            let static_db: &'static DB = &*db_ptr;
            static_db.snapshot()
        };
        // Balance Arc::into_raw to avoid leaking the temporary clone.
        unsafe {
            Arc::from_raw(db_ptr);
        }
        snapshot
    }

    fn read_options(&self) -> ReadOptions {
        provider::build_read_options(Some(&self.snapshot), &self.read_ahead_config)
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }

    fn pending_change(&self, key: &[u8]) -> Option<Option<Vec<u8>>> {
        self.pending_changes.lock().get(key).cloned()
    }

    fn merged_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
        let prefix = key_prefix.unwrap_or(&[]);
        let pending_changes = {
            let pending = self.pending_changes.lock();
            if pending.is_empty() {
                return None;
            }
            pending
                .iter()
                .filter(|(key, _)| key.starts_with(prefix))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Vec<_>>()
        };

        let mut items = BTreeMap::new();
        let iterator = provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            prefix,
            SeekDirection::Forward,
            &self.read_ahead_config,
        );
        for res in iterator {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(target: "neo", error = %err, "rocksdb iterator error");
                    continue;
                }
            };
            let key_vec = key.to_vec();
            if !prefix.is_empty() && !key_vec.starts_with(prefix) {
                break;
            }
            items.insert(key_vec, value.to_vec());
        }

        for (key, value) in pending_changes {
            if let Some(value) = value {
                items.insert(key, value);
            } else {
                items.remove(&key);
            }
        }

        let mut entries: Vec<_> = items.into_iter().collect();
        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        Some(entries)
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        if let Some(change) = self.pending_change(key.as_slice()) {
            return change;
        }

        self.db.get_opt(key, &self.read_options()).ok().flatten()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if let Some(items) = self.merged_entries(key_prefix.map(|k| k.as_slice()), direction) {
            return Box::new(items.into_iter());
        }

        if direction == SeekDirection::Backward {
            if let Some(prefix) = key_prefix {
                let iterator = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    Some(self.read_options()),
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                return Box::new(iterator.filter_map(|res| match res {
                    Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                    Err(err) => {
                        warn!(target: "neo", error = %err, "rocksdb iterator error");
                        None
                    }
                }));
            }
        }

        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = provider::iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            start,
            direction,
            &self.read_ahead_config,
        );
        Box::new(iterator.filter_map(|res| res.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        if let Some(change) = self.pending_change(raw.as_slice()) {
            let result = change.map(StorageItem::from_bytes);
            if let (Some(cache), Some(item)) = (&self.read_cache, &result) {
                let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                cache.put(key.clone(), item.clone(), size);
            }
            return result;
        }

        // Check read cache first (bloom filter will be checked inside)
        if let Some(ref cache) = self.read_cache {
            if let Some(item) = cache.get(key) {
                return Some(item);
            }
        }

        let result = self
            .db
            .get_opt(&raw, &self.read_options())
            .ok()
            .flatten()
            .map(StorageItem::from_bytes);

        // Cache the result if found and cache is configured
        if let (Some(cache), Some(item)) = (&self.read_cache, &result) {
            let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
            cache.put(key.clone(), item.clone(), size);
        }

        result
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        if let Some(items) = self.merged_entries(prefix_bytes.as_deref(), direction) {
            let read_cache = self.read_cache.clone();
            return Box::new(items.into_iter().map(move |(key_vec, value)| {
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value);
                if let Some(ref cache) = read_cache {
                    let size = storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    cache.put(storage_key.clone(), storage_item.clone(), size);
                }
                (storage_key, storage_item)
            }));
        }

        if direction == SeekDirection::Backward {
            if let Some(prefix) = prefix_bytes.as_ref() {
                let iter = provider::reverse_prefix_iterator(
                    self.db.as_ref(),
                    Some(self.read_options()),
                    prefix.as_slice(),
                    &self.read_ahead_config,
                );
                let read_cache = self.read_cache.clone();
                return Box::new(iter.filter_map(move |res| {
                    let (key, value) = match res {
                        Ok(entry) => entry,
                        Err(err) => {
                            warn!(target: "neo", error = %err, "rocksdb iterator error");
                            return None;
                        }
                    };
                    let key_vec: Vec<u8> = key.into();
                    let storage_key = StorageKey::from_bytes(&key_vec);
                    let storage_item = StorageItem::from_bytes(value.into());
                    if let Some(ref cache) = read_cache {
                        let size =
                            storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                        cache.put(storage_key.clone(), storage_item.clone(), size);
                    }
                    Some((storage_key, storage_item))
                }));
            }
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);

        // Create an iterator that also caches results
        let read_cache = self.read_cache.clone();

        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => {
                    warn!(target: "neo", error = %err, "rocksdb iterator error");
                    return None;
                }
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes {
                if !key_vec.starts_with(prefix) {
                    return None;
                }
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            let storage_item = StorageItem::from_bytes(value.into());

            // Cache the result
            if let Some(ref cache) = read_cache {
                let size = storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                cache.put(storage_key.clone(), storage_item.clone(), size);
            }

            Some((storage_key, storage_item))
        }))
    }
}

impl ReadOnlyStore for RocksDbSnapshot {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.write_batch.lock().delete(key.clone());
        self.pending_changes.lock().insert(key.clone(), None);
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> neo_storage::StorageResult<()> {
        self.write_batch.lock().put(key.clone(), value.clone());
        self.pending_changes
            .lock()
            .insert(key.clone(), Some(value.clone()));
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }
}

impl StoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> Arc<dyn Store> {
        self.store.clone() as Arc<dyn Store>
    }

    fn try_commit(&mut self) -> neo_storage::persistence::store_snapshot::SnapshotCommitResult {
        use neo_storage::StorageError;

        let mut batch_guard = self.write_batch.lock();
        let mut pending_guard = self.pending_changes.lock();

        if batch_guard.is_empty() {
            return Ok(());
        }

        let batch_data = batch_guard.data().to_vec();
        let pending_snapshot = pending_guard.clone();

        let mut batch = WriteBatch::default();
        mem::swap(&mut *batch_guard, &mut batch);
        pending_guard.clear();
        drop(pending_guard);
        drop(batch_guard);

        let _start = Instant::now();

        let mut write_opts = WriteOptions::default();
        let batch_config = self.store.batch_config.read();
        write_opts.set_sync(batch_config.sync_on_flush);
        if batch_config.disable_wal {
            write_opts.disable_wal(true);
        }
        drop(batch_config);

        if let Err(err) = self.db.write_opt(batch, &write_opts) {
            let mut batch_guard = self.write_batch.lock();
            let mut pending_guard = self.pending_changes.lock();
            *batch_guard = WriteBatch::from_data(&batch_data);
            *pending_guard = pending_snapshot;
            error!(target: "neo", error = %err, "rocksdb snapshot commit failed");
            return Err(StorageError::CommitFailed(format!(
                "RocksDB write failed: {}",
                err
            )));
        }

        // Point-invalidation is already performed in put() and delete(), so a full
        // cache.clear() is unnecessary here. Each mutated key was removed from the
        // read cache at mutation time, ensuring no stale entries survive the commit.

        Ok(())
    }
}

// These methods form the operational API for RocksDbStore (fast-sync, cache
// management, diagnostics).  The struct is crate-private so the compiler flags
// them as dead code, but they are intentionally kept for use by higher-level
// subsystems that will be wired up during node integration.
#[allow(dead_code)]
impl RocksDbStore {
    /// Enables fast sync mode optimizations (disable WAL, reduce fsync).
    pub fn enable_fast_sync_mode(&self) {
        // Switch to high-throughput batch config (disable WAL, no fsync)
        *self.batch_config.write() = WriteBatchConfig::high_throughput();

        if let Err(err) = self.db.set_options(&[("disable_auto_compactions", "true")]) {
            warn!(target: "neo", error = %err, "failed to disable auto compactions");
        }
        debug!(target: "neo", "enabled fast sync mode optimizations (WAL disabled, auto compaction disabled)");
    }

    /// Disables fast sync mode optimizations.
    pub fn disable_fast_sync_mode(&self) {
        // Restore balanced batch config (WAL enabled)
        *self.batch_config.write() = WriteBatchConfig::balanced();

        if let Err(err) = self
            .db
            .set_options(&[("disable_auto_compactions", "false")])
        {
            warn!(target: "neo", error = %err, "failed to enable auto compactions");
        }
        debug!(target: "neo", "disabled fast sync mode optimizations (WAL restored)");
    }

    /// Force flush all memtables to disk.
    pub fn flush_memtables(&self) {
        if let Err(err) = self.db.flush_wal(true) {
            warn!(target: "neo", error = %err, "failed to flush WAL");
        }
        if let Err(err) = self.db.flush_opt(&rocksdb::FlushOptions::default()) {
            warn!(target: "neo", error = %err, "failed to flush memtables");
        }
    }

    /// Returns memory usage statistics.
    pub fn memory_usage(&self) -> Option<(u64, u64)> {
        self.db
            .property_int_value("rocksdb.cur-size-active-mem-table")
            .ok()
            .flatten()
            .map(|active| {
                let total = self
                    .db
                    .property_int_value("rocksdb.cur-size-all-mem-tables")
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                (active, total)
            })
    }

    /// Enables read caching with the specified configuration.
    pub fn enable_read_cache(&mut self, config: ReadCacheConfig) {
        self.read_cache = Some(Arc::new(StorageReadCache::new(config)));
        debug!(target: "neo", "enabled read cache");
    }

    /// Disables read caching.
    pub fn disable_read_cache(&mut self) {
        self.read_cache = None;
        debug!(target: "neo", "disabled read cache");
    }

    /// Gets read cache statistics if caching is enabled.
    pub fn read_cache_stats(
        &self,
    ) -> Option<neo_storage::persistence::read_cache::ReadCacheStatsSnapshot> {
        self.read_cache.as_ref().map(|c| c.stats())
    }

    /// Clears the read cache.
    pub fn clear_read_cache(&self) {
        if let Some(ref cache) = self.read_cache {
            cache.clear();
            debug!(target: "neo", "read cache cleared");
        }
    }

    /// Returns batch commit statistics.
    pub fn batch_commit_stats(&self) -> WriteBatchStatsSnapshot {
        self.batch_committer.buffer.stats_snapshot()
    }

    /// Forces a flush of pending batch writes.
    pub fn flush_batch_writes(&self) -> StorageResult<()> {
        self.batch_committer.buffer.force_flush()
    }
}

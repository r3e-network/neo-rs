use crate::{
    error::{StorageError, StorageResult},
    persistence::{
        commit_profile::{StoreCommitProfiler, StoreCommitStage, StoreCommitTimer},
        read_cache::{ReadCacheConfig, StorageReadCache},
        read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
        seek_direction::SeekDirection,
        storage::StorageConfig,
        store::{OnNewSnapshotDelegate, Store},
        store_snapshot::StoreSnapshot,
        write_batch_buffer::{WriteBatchConfig, WriteBatchStatsSnapshot},
        write_store::WriteStore,
    },
    types::{StorageItem, StorageKey},
};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    DB, DBIteratorWithThreadMode, ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs, mem,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tracing::{debug, error, warn};

use super::provider::{self, BatchCommitter, ReadAheadConfig};

/// RocksDB-backed byte store implementing the workspace [`Store`]/[`WriteStore`]
/// traits, with write buffering, an optional point read cache, and
/// crash-safe snapshot commits.
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
        if let Some(parent) = config.path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(err) = fs::create_dir_all(parent)
        {
            warn!(
                target: "neo",
                path = %config.path.display(),
                error = %err,
                "failed to create RocksDB data directory"
            );
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

    /// Creates the same raw snapshot while preserving a caller's callback identity.
    /// Compatibility facades pass themselves; all handlers and snapshot state stay here.
    pub fn snapshot_for(&self, callback_store: &dyn Store) -> Arc<dyn StoreSnapshot> {
        self.snapshot_with_store(callback_store, |store| store)
    }

    /// Wraps only the exposed store handle, not the raw snapshot or commit state.
    /// This keeps compatibility downcasts and callback identity in upper layers.
    pub fn snapshot_with_store<F>(
        &self,
        callback_store: &dyn Store,
        wrap_store: F,
    ) -> Arc<dyn StoreSnapshot>
    where
        F: FnOnce(Arc<RocksDbStore>) -> Arc<dyn Store>,
    {
        let store_arc = Arc::new(self.clone());
        let store_view = wrap_store(Arc::clone(&store_arc));
        let mut snapshot = RocksDbSnapshot::new(
            self.db.clone(),
            store_arc,
            self.read_cache.clone(),
            self.read_ahead_config,
        );
        snapshot.store_view = store_view;
        let snapshot = Arc::new(snapshot);

        let handlers = self.on_new_snapshot.read();
        for handler in handlers.iter() {
            handler(callback_store, snapshot.clone());
        }

        snapshot
    }
}

/// R09: a RocksDB read error is unrecoverable for a deterministic state
/// machine — interpreting it as "key absent" lets state decisions run on
/// fabricated data. Fail fast instead of returning `None`.
fn fail_fast_read(op: &str, err: &rocksdb::Error) -> ! {
    error!(target: "neo", error = %err, op, "rocksdb read failed; failing fast");
    panic!("RocksDB {op} failed: {err}");
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        // Check read cache first
        // Note: Vec<u8> doesn't implement StorageKey, so we skip caching for raw bytes
        match self.db.get(key) {
            Ok(value) => value,
            Err(err) => fail_fast_read("get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if direction == SeekDirection::Backward
            && let Some(prefix) = key_prefix
        {
            let iterator = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                None,
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iterator.filter_map(|res| match res {
                Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                Err(err) => fail_fast_read("iterator", &err),
            }));
        }

        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        let prefix = key_prefix.cloned();
        let iterator = iterator.take_while(move |res| match res {
            Ok((key, _)) => prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iterator.filter_map(|res| match res {
            Ok((k, v)) => Some((k.to_vec(), v.to_vec())),
            Err(err) => fail_fast_read("iterator", &err),
        }))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        // Check read cache first (bloom filter will be checked inside)
        if let Some(ref cache) = self.read_cache
            && let Some(item) = cache.get(key)
        {
            return Some(item);
        }

        let raw = key.to_array();
        let result = match self.db.get(&raw) {
            Ok(Some(bytes)) => Some(StorageItem::from_bytes(bytes)),
            Ok(None) => None,
            Err(err) => fail_fast_read("get", &err),
        };

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

        if direction == SeekDirection::Backward
            && let Some(prefix) = prefix_bytes.as_ref()
        {
            let iter = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                None,
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            let read_cache = self.read_cache.clone();
            return Box::new(iter.map(move |res| {
                let (key, value) = match res {
                    Ok(entry) => entry,
                    Err(err) => fail_fast_read("iterator", &err),
                };
                let key_vec: Vec<u8> = key.into();
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value.into());
                if let Some(ref cache) = read_cache {
                    let size = storage_item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    cache.put(storage_key.clone(), storage_item.clone(), size);
                }
                (storage_key, storage_item)
            }));
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);
        let prefix_boundary = prefix_bytes.clone();
        let iter = iter.take_while(move |res| match res {
            Ok((key, _)) => prefix_boundary
                .as_ref()
                .is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });

        // Create an iterator that also caches results
        let read_cache = self.read_cache.clone();

        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => fail_fast_read("iterator", &err),
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes
                && !key_vec.starts_with(prefix)
            {
                return None;
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

impl RocksDbStore {
    /// Executes the existing direct delete through a shared backend handle.
    pub fn delete_raw(&self, key: Vec<u8>) -> StorageResult<()> {
        self.db.delete(&key).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb delete failed");
            StorageError::Io {
                message: format!("RocksDB delete failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }

    /// Executes the existing direct write without routing through batching.
    pub fn put_raw(&self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.db.put(&key, &value).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put failed");
            StorageError::Io {
                message: format!("RocksDB put failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }

    /// Executes the existing WAL-backed synchronous direct write.
    pub fn put_sync_raw(&self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.db.put_opt(&key, &value, &options).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb put_sync failed");
            StorageError::Io {
                message: format!("RocksDB put_sync failed: {}", err),
            }
        })?;
        if let Some(ref cache) = self.read_cache {
            cache.remove(&StorageKey::from_bytes(&key));
        }
        Ok(())
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.delete_raw(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.put_raw(key, value)
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.put_sync_raw(key, value)
    }
}

impl Store for RocksDbStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        self.snapshot_for(self)
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

    fn try_flush_durable(&self) -> StorageResult<()> {
        RocksDbStore::try_flush_durable(self)
    }

    fn supports_durable_writes(&self) -> bool {
        true
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

/// Overlay snapshot over a `RocksDbStore`: buffered writes staged in memory,
/// reads served from pending changes and a DB point-in-time, and an
/// all-or-nothing `try_commit`.
///
/// # Drop order invariant (R08)
///
/// `snapshot` MUST be released before `db` (the `Arc<DB>`) is freed.
/// `DbSnapshot::drop` calls `release_snapshot` on its parent `DB`; if the
/// `Arc` were released first and this were the last owner, the database would
/// be freed and the subsequent `release_snapshot` would be a use-after-free.
///
/// `snapshot` is wrapped in `ManuallyDrop` and released explicitly in the
/// `Drop` impl below, which runs before Rust auto-drops any other field.
/// This makes the invariant self-documenting and independent of field
/// declaration order.
#[allow(dead_code)] // Field order is intentional; see drop-order invariant above.
pub struct RocksDbSnapshot {
    /// Wrapped in `ManuallyDrop` so the explicit `Drop` impl can release it
    /// before `db` is freed. Do not reorder or unwrap without updating `Drop`.
    snapshot: mem::ManuallyDrop<DbSnapshot<'static>>,
    pending_changes: Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    has_pending_changes: AtomicBool,
    /// Optional read cache for this snapshot
    read_cache: Option<Arc<StorageReadCache>>,
    /// Read-ahead configuration
    read_ahead_config: ReadAheadConfig,
    db: Arc<DB>,
    store: Arc<RocksDbStore>,
    store_view: Arc<dyn Store>,
    commit_profiler: Option<Arc<dyn StoreCommitProfiler>>,
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
            snapshot: mem::ManuallyDrop::new(snapshot),
            pending_changes: Mutex::new(HashMap::new()),
            has_pending_changes: AtomicBool::new(false),
            read_cache,
            read_ahead_config,
            db,
            store_view: store.clone(),
            store,
            commit_profiler: None,
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
        provider::build_read_options(Some(&*self.snapshot), &self.read_ahead_config)
    }

    fn point_read_options(&self) -> ReadOptions {
        let mut options = ReadOptions::default();
        options.set_snapshot(&*self.snapshot);
        options.set_verify_checksums(self.read_ahead_config.verify_checksums);
        options.fill_cache(self.read_ahead_config.fill_cache);
        options
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
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return None;
        }
        self.pending_changes.lock().get(key).cloned()
    }

    fn merged_entries(
        &self,
        key_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return None;
        }
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

    fn commit_pending(
        &mut self,
        force_durable: bool,
    ) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        if !self.has_pending_changes.load(Ordering::Acquire) {
            return Ok(());
        }
        if self.pending_changes.get_mut().is_empty() {
            self.has_pending_changes.store(false, Ordering::Release);
            return Ok(());
        }

        let mut write_opts = WriteOptions::default();
        if force_durable {
            write_opts.set_sync(true);
        } else {
            let batch_config = self.store.batch_config.read();
            write_opts.set_sync(batch_config.sync_on_flush);
            if batch_config.disable_wal {
                write_opts.disable_wal(true);
            }
        }

        let batch = {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::WriteBatchBuild,
            );
            let mut batch = WriteBatch::default();
            for (key, value) in self.pending_changes.get_mut().iter() {
                match value {
                    Some(val) => batch.put(key, val),
                    None => batch.delete(key),
                }
            }
            batch
        };

        let write_result = {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::DatabaseWrite,
            );
            self.db.write_opt(batch, &write_opts)
        };
        if let Err(err) = write_result {
            error!(target: "neo", error = %err, durable = force_durable, "rocksdb snapshot commit failed");
            return Err(StorageError::CommitFailed(format!(
                "RocksDB write failed: {}",
                err
            )));
        }

        // Invalidate only after the atomic database write succeeds. Invalidating
        // before the write lets a concurrent latest-state read repopulate the
        // cache with the old value, which would then survive the commit.
        {
            let _stage = StoreCommitTimer::new(
                self.commit_profiler.as_deref(),
                StoreCommitStage::CacheInvalidation,
            );
            if let Some(ref cache) = self.read_cache {
                for key in self.pending_changes.get_mut().keys() {
                    cache.remove(&StorageKey::from_bytes(key));
                }
            }
        }
        self.pending_changes.get_mut().clear();
        self.has_pending_changes.store(false, Ordering::Release);
        Ok(())
    }
}

impl Drop for RocksDbSnapshot {
    fn drop(&mut self) {
        // SAFETY: Drop the RocksDB snapshot before releasing the Arc<DB>.
        // `snapshot` holds a `'static` borrow of the DB; `DbSnapshot::drop`
        // calls `release_snapshot` on the underlying rocksdb::DB. If `db`
        // (the Arc) were freed first and this were the last owner, the DB
        // allocation would be gone and `release_snapshot` would access freed
        // memory (use-after-free, R08). ManuallyDrop lets us call
        // `drop_in_place` here, before Rust auto-drops `db` and `store`.
        unsafe { mem::ManuallyDrop::drop(&mut self.snapshot) };
        // `db` and `store` are dropped subsequently in declaration order.
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        if let Some(change) = self.pending_change(key.as_slice()) {
            return change;
        }

        match self.db.get_opt(key, &self.point_read_options()) {
            Ok(value) => value,
            Err(err) => fail_fast_read("snapshot get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        if let Some(items) = self.merged_entries(key_prefix.map(|k| k.as_slice()), direction) {
            return Box::new(items.into_iter());
        }

        if direction == SeekDirection::Backward
            && let Some(prefix) = key_prefix
        {
            let iterator = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                Some(self.read_options()),
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iterator.filter_map(|res| match res {
                Ok((key, value)) => Some((key.to_vec(), value.to_vec())),
                Err(err) => fail_fast_read("snapshot iterator", &err),
            }));
        }

        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        let prefix = key_prefix.cloned();
        let iterator = iterator.take_while(move |res| match res {
            Ok((key, _)) => prefix.as_ref().is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iterator.filter_map(|res| match res {
            Ok((k, v)) => Some((k.to_vec(), v.to_vec())),
            Err(err) => fail_fast_read("snapshot iterator", &err),
        }))
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        if let Some(change) = self.pending_change(raw.as_slice()) {
            // R10: uncommitted values must never enter the shared
            // latest-state cache.
            return change.map(StorageItem::from_bytes);
        }

        // R10: a snapshot reads its own historical view. It must not consult
        // the shared cache (which holds latest-state values for keys mutated
        // after this snapshot was created) nor populate it with historical
        // values that post-commit readers would then observe.
        match self.db.get_opt(&raw, &self.point_read_options()) {
            Ok(Some(bytes)) => Some(StorageItem::from_bytes(bytes)),
            Ok(None) => None,
            Err(err) => fail_fast_read("snapshot get", &err),
        }
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        if let Some(items) = self.merged_entries(prefix_bytes.as_deref(), direction) {
            // R10: snapshot views must not populate the shared latest-state
            // read cache.
            return Box::new(items.into_iter().map(|(key_vec, value)| {
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value);
                (storage_key, storage_item)
            }));
        }

        if direction == SeekDirection::Backward
            && let Some(prefix) = prefix_bytes.as_ref()
        {
            let iter = provider::reverse_prefix_iterator(
                self.db.as_ref(),
                Some(self.read_options()),
                prefix.as_slice(),
                &self.read_ahead_config,
            );
            return Box::new(iter.map(move |res| {
                let (key, value) = match res {
                    Ok(entry) => entry,
                    Err(err) => fail_fast_read("snapshot iterator", &err),
                };
                let key_vec: Vec<u8> = key.into();
                let storage_key = StorageKey::from_bytes(&key_vec);
                let storage_item = StorageItem::from_bytes(value.into());
                (storage_key, storage_item)
            }));
        }

        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);
        let prefix_boundary = prefix_bytes.clone();
        let iter = iter.take_while(move |res| match res {
            Ok((key, _)) => prefix_boundary
                .as_ref()
                .is_none_or(|prefix| key.starts_with(prefix)),
            Err(_) => true,
        });
        Box::new(iter.filter_map(move |res| {
            let (key, value) = match res {
                Ok(entry) => entry,
                Err(err) => fail_fast_read("snapshot iterator", &err),
            };
            let key_vec: Vec<u8> = key.into();
            if let Some(prefix) = &prefix_bytes
                && !key_vec.starts_with(prefix)
            {
                return None;
            }
            let storage_key = StorageKey::from_bytes(&key_vec);
            let storage_item = StorageItem::from_bytes(value.into());
            Some((storage_key, storage_item))
        }))
    }
}

impl ReadOnlyStore for RocksDbSnapshot {}

impl WriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        self.pending_changes.get_mut().insert(key, None);
        self.has_pending_changes.store(true, Ordering::Release);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        self.pending_changes.get_mut().insert(key, Some(value));
        self.has_pending_changes.store(true, Ordering::Release);
        Ok(())
    }
}

impl StoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> Arc<dyn Store> {
        self.store_view.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        self.commit_pending(false)
    }

    fn set_commit_profiler(&mut self, profiler: Option<Arc<dyn StoreCommitProfiler>>) {
        self.commit_profiler = profiler;
    }

    fn try_commit_durable(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        self.commit_pending(true)
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
        *self.batch_config.write() = WriteBatchConfig::durable();

        if let Err(err) = self
            .db
            .set_options(&[("disable_auto_compactions", "false")])
        {
            warn!(target: "neo", error = %err, "failed to enable auto compactions");
        }
        debug!(target: "neo", "disabled fast sync mode optimizations (WAL restored)");
    }

    /// Flushes buffered writes and synchronizes the WAL without discarding it.
    /// Memtable flushing is unnecessary for WAL-backed crash durability.
    pub fn try_flush_durable(&self) -> StorageResult<()> {
        self.flush_batch_writes()?;
        self.db.flush_wal(true).map_err(|err| {
            StorageError::CommitFailed(format!("RocksDB durable WAL flush failed: {err}"))
        })
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
    ) -> Option<crate::persistence::read_cache::ReadCacheStatsSnapshot> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_snapshot_invalidates_latest_state_read_cache() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = StorageConfig {
            path: temp.path().join("cache-commit"),
            ..StorageConfig::default()
        };
        let mut store = RocksDbStore::open(
            &config,
            WriteBatchConfig::default(),
            &Some(ReadCacheConfig::default()),
            false,
            false,
        )
        .unwrap();
        store.enable_read_cache(ReadCacheConfig::default());
        let key = StorageKey::new(1, b"current".to_vec());

        let mut initial = store.snapshot();
        Arc::get_mut(&mut initial)
            .unwrap()
            .put(key.to_array(), b"old".to_vec())
            .unwrap();
        Arc::get_mut(&mut initial).unwrap().try_commit().unwrap();
        assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"old");

        let mut pending = store.snapshot();
        Arc::get_mut(&mut pending)
            .unwrap()
            .put(key.to_array(), b"new".to_vec())
            .unwrap();
        // Staging a write must not expose it or evict the valid committed value.
        assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"old");
        Arc::get_mut(&mut pending).unwrap().try_commit().unwrap();
        assert_eq!(store.try_get(&key).unwrap().value_bytes().as_ref(), b"new");
    }

    #[derive(Default)]
    struct RecordingProfiler(Mutex<Vec<StoreCommitStage>>);

    impl StoreCommitProfiler for RecordingProfiler {
        fn record(&self, stage: StoreCommitStage, _elapsed: std::time::Duration) {
            self.0.lock().push(stage);
        }
    }

    #[test]
    fn store_cache_profiling_observes_commit_without_changing_data() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = StorageConfig {
            path: temp.path().join("commit-profile"),
            ..StorageConfig::default()
        };
        let store = RocksDbStore::open(
            &config,
            WriteBatchConfig::default(),
            &Some(ReadCacheConfig::default()),
            false,
            false,
        )
        .unwrap();
        let store: Arc<dyn Store> = Arc::new(store);
        let profiler = Arc::new(RecordingProfiler::default());
        let mut cache = crate::persistence::StoreCache::new(Arc::clone(&store));
        cache.set_commit_profiler(Some(profiler.clone()));
        let first = StorageKey::new(1, b"first".to_vec());
        cache.add(first.clone(), StorageItem::from_bytes(b"one".to_vec()));

        cache.try_commit().unwrap();

        assert_eq!(
            store.try_get(&first).unwrap().value_bytes().as_ref(),
            b"one"
        );
        assert_eq!(
            profiler.0.lock().as_slice(),
            &[
                StoreCommitStage::WriterSnapshotOpen,
                StoreCommitStage::ChangeApplication,
                StoreCommitStage::WriteBatchBuild,
                StoreCommitStage::DatabaseWrite,
                StoreCommitStage::CacheInvalidation,
            ]
        );

        profiler.0.lock().clear();
        cache.set_commit_profiler(None);
        let second = StorageKey::new(1, b"second".to_vec());
        cache.add(second.clone(), StorageItem::from_bytes(b"two".to_vec()));
        cache.try_commit().unwrap();
        assert_eq!(
            store.try_get(&second).unwrap().value_bytes().as_ref(),
            b"two"
        );
        assert!(profiler.0.lock().is_empty());
        store.try_flush_durable().unwrap();
        assert_eq!(
            store.try_get(&first).unwrap().value_bytes().as_ref(),
            b"one"
        );
        assert_eq!(
            store.try_get(&second).unwrap().value_bytes().as_ref(),
            b"two"
        );
    }
}

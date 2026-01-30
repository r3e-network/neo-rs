//! RocksDB-backed `IStore` implementation with snapshot support.
use crate::{
    error::{CoreError, CoreResult},
    persistence::{
        i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
        i_store::{IStore, OnNewSnapshotDelegate},
        i_store_provider::IStoreProvider,
        i_store_snapshot::IStoreSnapshot,
        i_write_store::IWriteStore,
        read_cache::{ReadCacheConfig, StorageReadCache},
        seek_direction::SeekDirection,
        storage::{CompactionStrategy, CompressionAlgorithm, StorageConfig},
        write_batch_buffer::{WriteBatchBuffer, WriteBatchConfig, WriteBatchStatsSnapshot},
    },
    smart_contract::{StorageItem, StorageKey},
};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    BlockBasedOptions, Cache, DBIteratorWithThreadMode, Direction, IteratorMode, Options,
    ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions, DB,
};
use std::{fs, mem, path::PathBuf, sync::Arc, time::Instant};
use tracing::{debug, error, warn};

/// Re-export batch commit types from write_batch_buffer for backward compatibility.
pub use crate::persistence::write_batch_buffer::{
    WriteBatchConfig as BatchCommitConfig, WriteBatchStats as BatchCommitStats,
    WriteBatchStatsSnapshot as BatchCommitStatsSnapshot,
};

/// Enhanced batch committer using WriteBatchBuffer.
struct BatchCommitter {
    buffer: WriteBatchBuffer,
}

impl BatchCommitter {
    fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let buffer = WriteBatchBuffer::new(db, config);

        Self { buffer }
    }

    fn try_add(&self, batch: &mut WriteBatch) -> usize {
        let count = batch.len();
        if count == 0 {
            return 0;
        }

        // Merge the batch into our buffer
        struct BatchIterator<'a> {
            buffer: &'a WriteBatchBuffer,
        }

        impl<'a> rocksdb::WriteBatchIterator for BatchIterator<'a> {
            fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
                self.buffer.put(&key, &value);
            }

            fn delete(&mut self, key: Box<[u8]>) {
                self.buffer.delete(&key);
            }
        }

        let mut iter = BatchIterator {
            buffer: &self.buffer,
        };
        batch.iterate(&mut iter);

        count
    }

    fn flush(&self) -> Option<WriteBatch> {
        // The buffer flushes automatically, but we can force it here
        if self.buffer.has_pending() {
            if let Err(e) = self.buffer.force_flush() {
                error!(target: "neo", error = %e, "batch committer flush failed");
            }
        }
        None
    }
}

/// RocksDB-backed store provider compatible with Neo's `IStore`.
#[derive(Debug, Clone)]
pub struct RocksDBStoreProvider {
    base_config: StorageConfig,
    batch_config: BatchCommitConfig,
    batch_stats: Arc<BatchCommitStats>,
    /// Read cache configuration
    read_cache_config: Option<ReadCacheConfig>,
    /// Enable bloom filters for SST files
    enable_bloom_filters: bool,
    /// Enable read-ahead for sequential scans
    enable_read_ahead: bool,
}

impl RocksDBStoreProvider {
    pub fn new(base_config: StorageConfig) -> Self {
        Self {
            base_config,
            batch_config: BatchCommitConfig::default(),
            batch_stats: Arc::new(BatchCommitStats::new()),
            read_cache_config: Some(ReadCacheConfig::default()),
            enable_bloom_filters: true,
            enable_read_ahead: true,
        }
    }

    pub fn with_batch_config(mut self, config: BatchCommitConfig) -> Self {
        self.batch_config = config;
        self
    }

    pub fn with_read_cache(mut self, config: ReadCacheConfig) -> Self {
        self.read_cache_config = Some(config);
        self
    }

    pub fn without_read_cache(mut self) -> Self {
        self.read_cache_config = None;
        self
    }

    pub fn with_bloom_filters(mut self, enable: bool) -> Self {
        self.enable_bloom_filters = enable;
        self
    }

    pub fn with_read_ahead(mut self, enable: bool) -> Self {
        self.enable_read_ahead = enable;
        self
    }

    pub fn batch_stats(&self) -> Arc<BatchCommitStats> {
        Arc::clone(&self.batch_stats)
    }

    fn resolved_path(&self, override_path: &str) -> PathBuf {
        if override_path.is_empty() {
            self.base_config.path.clone()
        } else {
            PathBuf::from(override_path)
        }
    }
}

impl IStoreProvider for RocksDBStoreProvider {
    fn name(&self) -> &str {
        "RocksDBStore"
    }

    fn get_store(&self, path: &str) -> CoreResult<Arc<dyn IStore>> {
        let resolved = self.resolved_path(path);
        let config = StorageConfig {
            path: resolved,
            ..self.base_config.clone()
        };
        let store = RocksDbStore::open(
            &config,
            &self.read_cache_config,
            self.enable_bloom_filters,
            self.enable_read_ahead,
        )
        .map_err(|err| CoreError::Io {
            message: format!(
                "failed to open RocksDB store at {}: {err}",
                config.path.display()
            ),
        })?;
        Ok(Arc::new(store))
    }
}

/// Read-ahead configuration for sequential scans.
#[derive(Debug, Clone, Copy)]
pub struct ReadAheadConfig {
    /// Enable read-ahead
    pub enabled: bool,
    /// Read-ahead size in bytes
    pub read_ahead_size: usize,
    /// Verify checksums during iteration
    pub verify_checksums: bool,
    /// Fill cache during read-ahead
    pub fill_cache: bool,
}

impl Default for ReadAheadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            read_ahead_size: 256 * 1024, // 256KB
            verify_checksums: true,
            fill_cache: true,
        }
    }
}

struct RocksDbStore {
    db: Arc<DB>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
    batch_committer: Arc<BatchCommitter>,
    batch_config: WriteBatchConfig,
    /// Optional read cache for frequently accessed keys
    read_cache: Option<Arc<StorageReadCache>>,
    /// Read-ahead configuration
    read_ahead_config: ReadAheadConfig,
}

impl RocksDbStore {
    fn open(
        config: &StorageConfig,
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

        let options = build_db_options(config, enable_bloom_filters);
        let db = if config.read_only {
            Arc::new(DB::open_for_read_only(&options, &config.path, false)?)
        } else {
            Arc::new(DB::open(&options, &config.path)?)
        };

        let batch_committer = Arc::new(BatchCommitter::new(
            Arc::clone(&db),
            WriteBatchConfig::default(),
        ));

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
            batch_config: WriteBatchConfig::default(),
            read_cache,
            read_ahead_config,
        })
    }

    #[allow(dead_code)]
    fn fast_write_options() -> WriteOptions {
        let mut opts = WriteOptions::default();
        opts.set_sync(false);
        opts.disable_wal(true);
        opts
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        iterator_from(
            self.db.as_ref(),
            None,
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }

    #[allow(dead_code)]
    fn read_options(&self) -> ReadOptions {
        build_read_options(None, &self.read_ahead_config)
    }

    #[allow(dead_code)]
    pub fn flush_batch_commits(&self) {
        if let Some(batch) = self.batch_committer.flush() {
            if let Err(err) = self.db.write(batch) {
                error!(target: "neo", error = %err, "rocksdb batch flush failed");
            }
        }
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
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
        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        Box::new(iterator.filter_map(|res| res.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbStore {
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
        if let (Some(ref cache), Some(ref item)) = (&self.read_cache, &result) {
            let size = item.get_value().len() + std::mem::size_of::<StorageKey>();
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
                let size = storage_item.get_value().len() + std::mem::size_of::<StorageKey>();
                cache.put(storage_key.clone(), storage_item.clone(), size);
            }

            Some((storage_key, storage_item))
        }))
    }
}

impl IReadOnlyStore for RocksDbStore {}

impl IWriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn delete(&mut self, key: Vec<u8>) {
        if let Err(err) = self.db.delete(key) {
            warn!(target: "neo", error = %err, "rocksdb delete failed");
        }
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        if let Err(err) = self.db.put(key, value) {
            warn!(target: "neo", error = %err, "rocksdb put failed");
        }
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        if let Err(err) = self.db.put_opt(key, value, &options) {
            warn!(target: "neo", error = %err, "rocksdb put_sync failed");
        }
    }
}

impl IStore for RocksDbStore {
    fn get_snapshot(&self) -> Arc<dyn IStoreSnapshot> {
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
            batch_config: self.batch_config,
            read_cache: self.read_cache.clone(),
            read_ahead_config: self.read_ahead_config,
        }
    }
}

struct RocksDbSnapshot {
    store: Arc<RocksDbStore>,
    db: Arc<DB>,
    snapshot: DbSnapshot<'static>,
    write_batch: Mutex<WriteBatch>,
    batch_committer: Arc<BatchCommitter>,
    use_batch_commit: bool,
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
        let batch_committer = Arc::clone(&store.batch_committer);
        let use_batch_commit = store.batch_config.max_batch_size > 1;

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
            batch_committer,
            use_batch_commit,
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
        build_read_options(Some(&self.snapshot), &self.read_ahead_config)
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            key_or_prefix,
            direction,
            &self.read_ahead_config,
        )
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.db.get_opt(key, &self.read_options()).ok().flatten()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            start,
            direction,
            &self.read_ahead_config,
        );
        Box::new(iterator.filter_map(|res| res.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        // Check read cache first (bloom filter will be checked inside)
        if let Some(ref cache) = self.read_cache {
            if let Some(item) = cache.get(key) {
                return Some(item);
            }
        }

        let raw = key.to_array();
        let result = self
            .db
            .get_opt(&raw, &self.read_options())
            .ok()
            .flatten()
            .map(StorageItem::from_bytes);

        // Cache the result if found and cache is configured
        if let (Some(ref cache), Some(ref item)) = (&self.read_cache, &result) {
            let size = item.get_value().len() + std::mem::size_of::<StorageKey>();
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
                let size = storage_item.get_value().len() + std::mem::size_of::<StorageKey>();
                cache.put(storage_key.clone(), storage_item.clone(), size);
            }

            Some((storage_key, storage_item))
        }))
    }
}

impl IReadOnlyStore for RocksDbSnapshot {}

impl IWriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn delete(&mut self, key: Vec<u8>) {
        self.write_batch.lock().delete(key);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.write_batch.lock().put(key, value);
    }
}

impl IStoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> Arc<dyn IStore> {
        self.store.clone() as Arc<dyn IStore>
    }

    fn try_commit(&mut self) -> crate::persistence::i_store_snapshot::SnapshotCommitResult {
        use crate::persistence::storage::StorageError;

        let mut batch_guard = self.write_batch.lock();

        if batch_guard.is_empty() {
            return Ok(());
        }

        let ops = batch_guard.len();
        let _start = Instant::now();

        if self.use_batch_commit {
            self.batch_committer.try_add(&mut batch_guard);
            return Ok(());
        }

        let mut batch = WriteBatch::default();
        mem::swap(&mut *batch_guard, &mut batch);
        drop(batch_guard);

        let mut write_opts = WriteOptions::default();
        if ops > 10 {
            write_opts.set_sync(false);
            write_opts.disable_wal(true);
        }

        self.db.write_opt(batch, &write_opts).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb snapshot commit failed");
            StorageError::CommitFailed(format!("RocksDB write failed: {}", err))
        })?;

        Ok(())
    }
}

impl RocksDbStore {
    /// Enables fast sync mode optimizations (disable WAL, reduce fsync).
    #[allow(dead_code)]
    pub fn enable_fast_sync_mode(&self) {
        let mut opts = Options::default();
        opts.set_disable_auto_compactions(true);
        if let Err(err) = self.db.set_options(&[("disable_auto_compactions", "true")]) {
            warn!(target: "neo", error = %err, "failed to disable auto compactions");
        }
        debug!(target: "neo", "enabled fast sync mode optimizations");
    }

    /// Disables fast sync mode optimizations.
    #[allow(dead_code)]
    pub fn disable_fast_sync_mode(&self) {
        if let Err(err) = self
            .db
            .set_options(&[("disable_auto_compactions", "false")])
        {
            warn!(target: "neo", error = %err, "failed to enable auto compactions");
        }
        debug!(target: "neo", "disabled fast sync mode optimizations");
    }

    /// Force flush all memtables to disk.
    #[allow(dead_code)]
    pub fn flush_memtables(&self) {
        if let Err(err) = self.db.flush_wal(true) {
            warn!(target: "neo", error = %err, "failed to flush WAL");
        }
        if let Err(err) = self.db.flush_opt(&rocksdb::FlushOptions::default()) {
            warn!(target: "neo", error = %err, "failed to flush memtables");
        }
    }

    /// Returns memory usage statistics.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub fn enable_read_cache(&mut self, config: ReadCacheConfig) {
        self.read_cache = Some(Arc::new(StorageReadCache::new(config)));
        debug!(target: "neo", "enabled read cache");
    }

    /// Disables read caching.
    #[allow(dead_code)]
    pub fn disable_read_cache(&mut self) {
        self.read_cache = None;
        debug!(target: "neo", "disabled read cache");
    }

    /// Gets read cache statistics if caching is enabled.
    #[allow(dead_code)]
    pub fn read_cache_stats(
        &self,
    ) -> Option<crate::persistence::read_cache::ReadCacheStatsSnapshot> {
        self.read_cache.as_ref().map(|c| c.stats())
    }

    /// Clears the read cache.
    #[allow(dead_code)]
    pub fn clear_read_cache(&self) {
        if let Some(ref cache) = self.read_cache {
            cache.clear();
            debug!(target: "neo", "read cache cleared");
        }
    }

    /// Returns batch commit statistics.
    #[allow(dead_code)]
    pub fn batch_commit_stats(&self) -> WriteBatchStatsSnapshot {
        self.batch_committer.buffer.stats_snapshot()
    }

    /// Forces a flush of pending batch writes.
    #[allow(dead_code)]
    pub fn flush_batch_writes(&self) -> CoreResult<()> {
        self.batch_committer.buffer.force_flush()
    }
}

/// Build read options with read-ahead configuration.
fn build_read_options(
    snapshot: Option<&DbSnapshot>,
    read_ahead_config: &ReadAheadConfig,
) -> ReadOptions {
    let mut options = ReadOptions::default();

    if let Some(snap) = snapshot {
        options.set_snapshot(snap);
    }

    if read_ahead_config.enabled {
        // Enable read-ahead for sequential scans
        options.set_readahead_size(read_ahead_config.read_ahead_size);
    }

    options.set_verify_checksums(read_ahead_config.verify_checksums);
    options.fill_cache(read_ahead_config.fill_cache);

    options
}

fn iterator_from<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    key_or_prefix: &[u8],
    direction: SeekDirection,
    read_ahead_config: &ReadAheadConfig,
) -> DBIteratorWithThreadMode<'a, DB> {
    let mode = match direction {
        SeekDirection::Forward => {
            if key_or_prefix.is_empty() {
                IteratorMode::Start
            } else {
                IteratorMode::From(key_or_prefix, Direction::Forward)
            }
        }
        SeekDirection::Backward => {
            if key_or_prefix.is_empty() {
                IteratorMode::End
            } else {
                IteratorMode::From(key_or_prefix, Direction::Reverse)
            }
        }
    };

    let opts = read_options.unwrap_or_else(|| build_read_options(None, read_ahead_config));
    db.iterator_opt(mode, opts)
}

fn build_db_options(config: &StorageConfig, enable_bloom_filters: bool) -> Options {
    let mut options = Options::default();
    options.create_if_missing(true);
    options.set_error_if_exists(false);
    options.set_compression_type(match config.compression_algorithm {
        CompressionAlgorithm::None => rocksdb::DBCompressionType::None,
        CompressionAlgorithm::Lz4 => rocksdb::DBCompressionType::Lz4,
        CompressionAlgorithm::Zstd => rocksdb::DBCompressionType::Zstd,
    });

    match config.compaction_strategy {
        CompactionStrategy::Level => {
            options.set_compaction_style(rocksdb::DBCompactionStyle::Level)
        }
        CompactionStrategy::Universal => {
            options.set_compaction_style(rocksdb::DBCompactionStyle::Universal)
        }
        CompactionStrategy::Fifo => options.set_compaction_style(rocksdb::DBCompactionStyle::Fifo),
    }

    if let Some(max_open) = config.max_open_files {
        options.set_max_open_files(max_open as i32);
    } else {
        options.set_max_open_files(4000);
    }

    options.set_max_background_jobs(16);
    options.set_bytes_per_sync(0);
    // Enable optimize_filters_for_hits when bloom filters are enabled
    // This reduces filter block size at the cost of slightly more disk seeks on negative lookups
    options.set_optimize_filters_for_hits(enable_bloom_filters);

    if let Some(write_buffer) = config.write_buffer_size {
        options.set_write_buffer_size(write_buffer);
    } else {
        options.set_write_buffer_size(64 * 1024 * 1024);
    }
    options.set_max_write_buffer_number(4);
    options.set_min_write_buffer_number_to_merge(2);

    // Configure block cache and bloom filters
    let cache_size = config.cache_size.unwrap_or(256 * 1024 * 1024);
    let cache = Cache::new_lru_cache(cache_size);
    let mut table_options = BlockBasedOptions::default();
    table_options.set_block_cache(&cache);

    if enable_bloom_filters {
        // Enable bloom filters with 10 bits per key for ~1% false positive rate
        table_options.set_bloom_filter(10.0, false);
        // Cache index and filter blocks in block cache
        table_options.set_cache_index_and_filter_blocks(true);
        table_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
    }

    options.set_block_based_table_factory(&table_options);

    if config.enable_statistics {
        options.enable_statistics();
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn opens_store_and_creates_directory() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("rocksdb");
        let cfg = StorageConfig {
            path: db_path.clone(),
            ..Default::default()
        };

        let provider = RocksDBStoreProvider::new(cfg);
        let store = provider
            .get_store(db_path.to_str().unwrap())
            .expect("rocksdb store");
        assert!(db_path.exists(), "db path should be created");

        // basic snapshot call to ensure the store is usable
        let _snapshot = store.get_snapshot();
    }

    #[test]
    fn returns_error_when_path_is_file() {
        let tmp = TempDir::new().expect("tempdir");
        let file_path = tmp.path().join("not-a-dir");
        fs::write(&file_path, b"content").expect("write file");

        let cfg = StorageConfig {
            path: file_path.clone(),
            ..Default::default()
        };
        let provider = RocksDBStoreProvider::new(cfg);

        let result = provider.get_store(file_path.to_str().unwrap());
        match result {
            Ok(_) => panic!("expected failure when path is a file"),
            Err(err) => {
                assert!(
                    err.to_string()
                        .to_ascii_lowercase()
                        .contains("failed to open rocksdb store"),
                    "unexpected error: {err}"
                );
            }
        }
    }
}

//! RocksDB-backed `IStore` implementation with snapshot support.
use crate::{
    error::{CoreError, CoreResult},
    persistence::{
        i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
        i_store::{IStore, OnNewSnapshotDelegate},
        i_store_provider::IStoreProvider,
        i_store_snapshot::IStoreSnapshot,
        i_write_store::IWriteStore,
        seek_direction::SeekDirection,
        storage::{CompactionStrategy, CompressionAlgorithm, StorageConfig},
    },
    smart_contract::{StorageItem, StorageKey},
};
use parking_lot::{Mutex, RwLock};
use rocksdb::{
    BlockBasedOptions, Cache, DBIteratorWithThreadMode, Direction, IteratorMode, Options,
    ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions, DB,
};
use std::{
    fs, mem,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};
use tracing::{debug, error, warn};

/// Batch commit statistics for monitoring.
#[derive(Debug, Default)]
pub struct BatchCommitStats {
    pub total_commits: AtomicUsize,
    pub total_batches: AtomicUsize,
    pub total_operations: AtomicUsize,
    pub total_duration_ms: AtomicUsize,
    pub max_batch_size: AtomicUsize,
}

impl BatchCommitStats {
    pub fn new() -> Self {
        Self {
            total_commits: AtomicUsize::new(0),
            total_batches: AtomicUsize::new(0),
            total_operations: AtomicUsize::new(0),
            total_duration_ms: AtomicUsize::new(0),
            max_batch_size: AtomicUsize::new(0),
        }
    }

    pub fn record_commit(&self, operations: usize, duration_ms: u64) {
        self.total_commits.fetch_add(1, Ordering::Relaxed);
        self.total_operations
            .fetch_add(operations, Ordering::Relaxed);
        self.total_duration_ms
            .fetch_add(duration_ms as usize, Ordering::Relaxed);

        loop {
            let current_max = self.max_batch_size.load(Ordering::Relaxed);
            if operations <= current_max {
                break;
            }
            if self
                .max_batch_size
                .compare_exchange(
                    current_max,
                    operations,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    pub fn record_batch(&self) {
        self.total_batches.fetch_add(1, Ordering::Relaxed);
    }

    pub fn stats(&self) -> (usize, usize, usize, u64, usize) {
        (
            self.total_commits.load(Ordering::Relaxed),
            self.total_batches.load(Ordering::Relaxed),
            self.total_operations.load(Ordering::Relaxed),
            self.total_duration_ms.load(Ordering::Relaxed) as u64,
            self.max_batch_size.load(Ordering::Relaxed),
        )
    }
}

/// Batch commit configuration.
#[derive(Debug, Clone)]
pub struct BatchCommitConfig {
    pub enabled: bool,
    pub max_batch_size: usize,
    pub max_delay_ms: u64,
    pub min_operations: usize,
}

impl Default for BatchCommitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_batch_size: 100,
            max_delay_ms: 50,
            min_operations: 10,
        }
    }
}

/// Batch commit accumulator for fast sync mode.
struct BatchCommitter {
    config: BatchCommitConfig,
    stats: Arc<BatchCommitStats>,
    pending_batch: Mutex<WriteBatch>,
    last_flush: AtomicUsize,
    pending_operations: AtomicUsize,
}

impl BatchCommitter {
    fn new(config: BatchCommitConfig, stats: Arc<BatchCommitStats>) -> Self {
        Self {
            config,
            stats,
            pending_batch: Mutex::new(WriteBatch::default()),
            last_flush: AtomicUsize::new(0),
            pending_operations: AtomicUsize::new(0),
        }
    }

    fn try_add(&self, batch: &mut WriteBatch) -> usize {
        let count = batch.len();
        if count == 0 {
            return 0;
        }

        if !self.config.enabled {
            return count;
        }

        let mut pending = self.pending_batch.lock();
        let pending_ops = self.pending_operations.load(Ordering::Relaxed);
        let total = pending_ops + count;

        if total >= self.config.max_batch_size {
            self.flush_locked(&mut pending);
            let remaining = self.config.max_batch_size.saturating_sub(count);
            if remaining > 0 && count <= remaining {
                Self::merge_batches(&mut pending, batch);
                self.pending_operations.store(count, Ordering::Relaxed);
                return 0;
            }
        }

        Self::merge_batches(&mut pending, batch);
        self.pending_operations.store(total, Ordering::Relaxed);
        count
    }

    fn merge_batches(dest: &mut WriteBatch, src: &WriteBatch) {
        struct BatchIterator<'a> {
            dest: &'a mut WriteBatch,
        }

        impl<'a> rocksdb::WriteBatchIterator for BatchIterator<'a> {
            fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
                self.dest.put(&key, &value);
            }

            fn delete(&mut self, key: Box<[u8]>) {
                self.dest.delete(&key);
            }
        }

        let mut iter = BatchIterator { dest };
        src.iterate(&mut iter);
    }

    fn flush_locked(&self, pending: &mut WriteBatch) {
        if pending.is_empty() {
            return;
        }
        self.stats.record_batch();
        *pending = WriteBatch::default();
        self.pending_operations.store(0, Ordering::Relaxed);
        self.last_flush.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as usize,
            Ordering::Relaxed,
        );
    }

    #[allow(dead_code)]
    fn should_flush(&self, force: bool) -> bool {
        if !self.config.enabled {
            return false;
        }

        if force {
            return !self.pending_operations.load(Ordering::Relaxed) == 0;
        }

        let ops = self.pending_operations.load(Ordering::Relaxed);
        if ops < self.config.min_operations {
            return false;
        }

        let last_flush = self.last_flush.load(Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if last_flush == 0 {
            return true;
        }

        now.saturating_sub(last_flush as u64) >= self.config.max_delay_ms
    }

    #[allow(dead_code)]
    fn flush(&self) -> Option<WriteBatch> {
        if !self.config.enabled {
            return None;
        }

        let mut pending = self.pending_batch.lock();
        if pending.is_empty() {
            return None;
        }

        let batch = std::mem::take(&mut *pending);
        self.pending_operations.store(0, Ordering::Relaxed);
        self.last_flush.store(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as usize,
            Ordering::Relaxed,
        );
        Some(batch)
    }

    #[allow(dead_code)]
    fn pending_count(&self) -> usize {
        self.pending_operations.load(Ordering::Relaxed)
    }
}

/// RocksDB-backed store provider compatible with Neo's `IStore`.
#[derive(Debug, Clone)]
pub struct RocksDBStoreProvider {
    base_config: StorageConfig,
    batch_config: BatchCommitConfig,
    batch_stats: Arc<BatchCommitStats>,
}

impl RocksDBStoreProvider {
    pub fn new(base_config: StorageConfig) -> Self {
        Self {
            base_config,
            batch_config: BatchCommitConfig::default(),
            batch_stats: Arc::new(BatchCommitStats::new()),
        }
    }

    pub fn with_batch_config(mut self, config: BatchCommitConfig) -> Self {
        self.batch_config = config;
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
        let store = RocksDbStore::open(&config).map_err(|err| CoreError::Io {
            message: format!(
                "failed to open RocksDB store at {}: {err}",
                config.path.display()
            ),
        })?;
        Ok(Arc::new(store))
    }
}

struct RocksDbStore {
    db: Arc<DB>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
    batch_committer: Arc<BatchCommitter>,
    batch_config: BatchCommitConfig,
}

impl RocksDbStore {
    fn open(config: &StorageConfig) -> Result<Self, rocksdb::Error> {
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

        let options = build_db_options(config);
        let db = if config.read_only {
            Arc::new(DB::open_for_read_only(&options, &config.path, false)?)
        } else {
            Arc::new(DB::open(&options, &config.path)?)
        };

        let batch_stats = Arc::new(BatchCommitStats::new());
        let batch_committer = Arc::new(BatchCommitter::new(
            BatchCommitConfig::default(),
            Arc::clone(&batch_stats),
        ));

        Ok(Self {
            db,
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
            batch_committer,
            batch_config: BatchCommitConfig::default(),
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
        iterator_from(self.db.as_ref(), None, key_or_prefix, direction)
    }

    #[allow(dead_code)]
    pub fn flush_batch_commits(&self) {
        if let Some(batch) = self.batch_committer.flush() {
            let start = Instant::now();
            if let Err(err) = self.db.write(batch) {
                error!(target: "neo", error = %err, "rocksdb batch flush failed");
            } else {
                let duration_ms = start.elapsed().as_millis() as u64;
                debug!(target: "neo", duration_ms, "rocksdb batch flush completed");
            }
        }
    }

    #[allow(dead_code)]
    pub fn batch_commit_stats(&self) -> (usize, usize, usize, u64, usize) {
        self.batch_committer.stats.stats()
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
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
        let raw = key.to_array();
        self.db.get(raw).ok().flatten().map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, direction);
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
            Some((
                StorageKey::from_bytes(&key_vec),
                StorageItem::from_bytes(value.into()),
            ))
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
        let snapshot = Arc::new(RocksDbSnapshot::new(self.db.clone(), store_arc));

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
            batch_config: self.batch_config.clone(),
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
}

impl RocksDbSnapshot {
    fn new(db: Arc<DB>, store: Arc<RocksDbStore>) -> Self {
        let snapshot = Self::create_snapshot(&db);
        let batch_committer = Arc::clone(&store.batch_committer);
        let use_batch_commit = store.batch_config.enabled && store.batch_config.max_batch_size > 1;

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
            batch_committer,
            use_batch_commit,
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
        let mut options = ReadOptions::default();
        options.set_snapshot(&self.snapshot);
        options
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
        );
        Box::new(iterator.filter_map(|res| res.ok().map(|(k, v)| (k.to_vec(), v.to_vec()))))
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for RocksDbSnapshot {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        self.db
            .get_opt(&raw, &self.read_options())
            .ok()
            .flatten()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = iterator_from(
            self.db.as_ref(),
            Some(self.read_options()),
            start,
            direction,
        );
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
            Some((
                StorageKey::from_bytes(&key_vec),
                StorageItem::from_bytes(value.into()),
            ))
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
        let start = Instant::now();

        if self.use_batch_commit {
            self.batch_committer.try_add(&mut batch_guard);
            let duration_ms = start.elapsed().as_millis() as u64;
            self.batch_committer.stats.record_commit(ops, duration_ms);
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
}

fn iterator_from<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    key_or_prefix: &[u8],
    direction: SeekDirection,
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

    match read_options {
        Some(opts) => db.iterator_opt(mode, opts),
        None => db.iterator(mode),
    }
}

fn build_db_options(config: &StorageConfig) -> Options {
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
    options.set_optimize_filters_for_hits(false);

    if let Some(write_buffer) = config.write_buffer_size {
        options.set_write_buffer_size(write_buffer);
    } else {
        options.set_write_buffer_size(64 * 1024 * 1024);
    }
    options.set_max_write_buffer_number(4);
    options.set_min_write_buffer_number_to_merge(2);

    if let Some(cache_size) = config.cache_size {
        let cache = Cache::new_lru_cache(cache_size);
        let mut table_options = BlockBasedOptions::default();
        table_options.set_block_cache(&cache);
        options.set_block_based_table_factory(&table_options);
    } else {
        let cache = Cache::new_lru_cache(256 * 1024 * 1024);
        let mut table_options = BlockBasedOptions::default();
        table_options.set_block_cache(&cache);
        options.set_block_based_table_factory(&table_options);
    }

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

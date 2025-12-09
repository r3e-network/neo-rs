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
use tracing::error;
use rocksdb::{
    BlockBasedOptions, Cache, DBIteratorWithThreadMode, Direction, IteratorMode, Options,
    ReadOptions, Snapshot as DbSnapshot, WriteBatch, WriteOptions, DB,
};
use std::{
    fs, mem,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
};
use tracing::warn;

/// RocksDB-backed store provider compatible with Neo's `IStore`.
#[derive(Debug, Clone)]
pub struct RocksDBStoreProvider {
    base_config: StorageConfig,
}

impl RocksDBStoreProvider {
    pub fn new(base_config: StorageConfig) -> Self {
        Self { base_config }
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
        let mut config = self.base_config.clone();
        config.path = resolved;
        let store = RocksDbStore::open(&config).map_err(|err| CoreError::Io {
            message: format!(
                "failed to open RocksDB store at {}: {err}",
                config.path.display()
            ),
        })?;
        Ok(Arc::new(store))
    }
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
        let mut cfg = StorageConfig::default();
        cfg.path = db_path.clone();

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

        let mut cfg = StorageConfig::default();
        cfg.path = file_path.clone();
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

struct RocksDbStore {
    db: Arc<DB>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
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

        Ok(Self {
            db,
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        })
    }

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        direction: SeekDirection,
    ) -> DBIteratorWithThreadMode<'_, DB> {
        iterator_from(self.db.as_ref(), None, key_or_prefix, direction)
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

        let handlers = self.on_new_snapshot.read().unwrap();
        for handler in handlers.iter() {
            handler(self, snapshot.clone());
        }

        snapshot
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.on_new_snapshot.write().unwrap().push(handler);
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
        }
    }
}

struct RocksDbSnapshot {
    store: Arc<RocksDbStore>,
    db: Arc<DB>,
    snapshot: DbSnapshot<'static>,
    write_batch: Mutex<WriteBatch>,
}

impl RocksDbSnapshot {
    fn new(db: Arc<DB>, store: Arc<RocksDbStore>) -> Self {
        let snapshot = Self::create_snapshot(&db);

        Self {
            store,
            db,
            snapshot,
            write_batch: Mutex::new(WriteBatch::default()),
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
        self.write_batch.lock().unwrap().delete(key);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.write_batch.lock().unwrap().put(key, value);
    }
}

impl IStoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> Arc<dyn IStore> {
        self.store.clone() as Arc<dyn IStore>
    }

    fn try_commit(&mut self) -> crate::persistence::i_store_snapshot::SnapshotCommitResult {
        use crate::persistence::storage::StorageError;

        let mut batch_guard = self.write_batch.lock().map_err(|e| {
            StorageError::CommitFailed(format!("Failed to acquire lock: {}", e))
        })?;

        if batch_guard.is_empty() {
            return Ok(());
        }

        let mut batch = WriteBatch::default();
        mem::swap(&mut *batch_guard, &mut batch);
        drop(batch_guard);

        self.db.write(batch).map_err(|err| {
            error!(target: "neo", error = %err, "rocksdb snapshot commit failed");
            StorageError::CommitFailed(format!("RocksDB write failed: {}", err))
        })?;

        Ok(())
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
    }

    if let Some(write_buffer) = config.write_buffer_size {
        options.set_write_buffer_size(write_buffer);
    }

    if let Some(cache_size) = config.cache_size {
        let cache = Cache::new_lru_cache(cache_size);
        let mut table_options = BlockBasedOptions::default();
        table_options.set_block_cache(&cache);
        options.set_block_based_table_factory(&table_options);
    }

    if config.enable_statistics {
        options.enable_statistics();
    }

    options
}

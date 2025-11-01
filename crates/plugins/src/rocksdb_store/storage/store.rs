use super::options;
use super::snapshot::Snapshot;
use neo_core::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use neo_core::persistence::i_store::{IStore, OnNewSnapshotDelegate};
use neo_core::persistence::i_store_provider::IStoreProvider;
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::persistence::i_write_store::IWriteStore;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::persistence::store_factory::StoreFactory;
use rocksdb::{DBIteratorWithThreadMode, IteratorMode, WriteOptions, DB};
use std::path::Path;
use std::sync::{Arc, RwLock};

/// RocksDB-backed implementation of `IStore` mirroring Neo.Plugins.Storage.Store.
#[derive(Clone)]
pub struct Store {
    db: Arc<DB>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
}

impl Store {
    pub fn open<P: AsRef<Path>>(path: P) -> Self {
        let absolute_path = if path.as_ref().is_absolute() {
            path.as_ref().to_path_buf()
        } else {
            std::env::current_dir()
                .expect("Failed to resolve current directory")
                .join(path.as_ref())
        };

        if let Some(parent) = absolute_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).expect("Failed to create RocksDB directory");
            }
        }

        let options = options::db_options();
        let db = Arc::new(DB::open(&options, &absolute_path).expect("Failed to open RocksDB"));

        Self {
            db,
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }

    fn iterator_from(&self, key_or_prefix: &[u8], direction: SeekDirection) -> StoreIterator<'_> {
        StoreIterator::new(self.db.as_ref(), key_or_prefix, direction)
    }

    fn write(&self, key: &[u8], value: &[u8], write_options: &WriteOptions) {
        self.db
            .put_opt(key, value, write_options)
            .expect("Failed to write to RocksDB");
    }

    fn delete_internal(&self, key: &[u8], write_options: &WriteOptions) {
        self.db
            .delete_opt(key, write_options)
            .expect("Failed to delete from RocksDB");
    }
}

struct StoreIterator<'a> {
    inner: DBIteratorWithThreadMode<'a, DB>,
    direction: SeekDirection,
}

impl<'a> StoreIterator<'a> {
    fn new(db: &'a DB, key_or_prefix: &[u8], direction: SeekDirection) -> Self {
        let mode = match direction {
            SeekDirection::Forward => {
                if key_or_prefix.is_empty() {
                    IteratorMode::Start
                } else {
                    IteratorMode::From(key_or_prefix, rocksdb::Direction::Forward)
                }
            }
            SeekDirection::Backward => {
                if key_or_prefix.is_empty() {
                    IteratorMode::End
                } else {
                    IteratorMode::From(key_or_prefix, rocksdb::Direction::Reverse)
                }
            }
        };

        let inner = db.iterator_opt(mode, options::read_options());
        Self { inner, direction }
    }
}

impl<'a> Iterator for StoreIterator<'a> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if !self.inner.valid() {
            return None;
        }

        let key = self.inner.key().to_vec();
        let value = self.inner.value().to_vec();

        match self.direction {
            SeekDirection::Forward => {
                self.inner.next();
            }
            SeekDirection::Backward => {
                self.inner.prev();
            }
        }

        Some((key, value))
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for Store {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.db.get(key).expect("RocksDB get failed")
    }

    fn contains(&self, key: &Vec<u8>) -> bool {
        self.db
            .get_pinned(key)
            .expect("RocksDB contains check failed")
            .is_some()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let start = key_prefix.map(|k| k.as_slice()).unwrap_or(&[]);
        let iterator = self.iterator_from(start, direction);
        Box::new(iterator)
    }
}

impl IWriteStore<Vec<u8>, Vec<u8>> for Store {
    fn delete(&mut self, key: Vec<u8>) {
        let write_options = options::write_options();
        self.delete_internal(&key, &write_options);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let write_options = options::write_options();
        self.write(&key, &value, &write_options);
    }

    fn put_sync(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let write_options = options::write_options_sync();
        self.write(&key, &value, &write_options);
    }
}

impl IStore for Store {
    fn get_snapshot(&self) -> Arc<dyn IStoreSnapshot> {
        let store_clone = Arc::new(self.clone());
        let snapshot = Snapshot::new(self.db.clone(), store_clone.clone());
        let snapshot_arc: Arc<dyn IStoreSnapshot> = Arc::new(snapshot);

        let handlers = self.on_new_snapshot.read().unwrap();
        for handler in handlers.iter() {
            handler(self, Arc::clone(&snapshot_arc));
        }

        snapshot_arc
    }

    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
        self.on_new_snapshot.write().unwrap().push(handler);
    }
}

/// RocksDB store provider registering with `StoreFactory`.
pub struct RocksDBStoreProvider;

impl RocksDBStoreProvider {
    pub fn new() -> Self {
        Self
    }

    pub fn register() {
        StoreFactory::register_provider(Arc::new(Self::new()));
    }
}

impl Default for RocksDBStoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IStoreProvider for RocksDBStoreProvider {
    fn name(&self) -> &str {
        "RocksDBStore"
    }

    fn get_store(&self, path: &str) -> Arc<dyn IStore> {
        Arc::new(Store::open(path))
    }
}

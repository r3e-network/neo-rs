use super::options;
use super::snapshot::Snapshot;
use neo_core::persistence::i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric};
use neo_core::persistence::i_store::{IStore, OnNewSnapshotDelegate};
use neo_core::persistence::i_store_provider::IStoreProvider;
use neo_core::persistence::i_store_snapshot::IStoreSnapshot;
use neo_core::persistence::i_write_store::IWriteStore;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::persistence::store_factory::StoreFactory;
use neo_core::smart_contract::{StorageItem, StorageKey};
use rocksdb::{DBIteratorWithThreadMode, IteratorMode, WriteOptions, DB};
use std::any::Any;
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

    fn iterator_from(
        &self,
        key_or_prefix: &[u8],
        prefix: Option<Vec<u8>>,
        direction: SeekDirection,
    ) -> StoreIterator<'_> {
        StoreIterator::new(self.db.as_ref(), key_or_prefix, prefix, direction)
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
    prefix: Option<Vec<u8>>,
    done: bool,
}

impl<'a> StoreIterator<'a> {
    fn new(
        db: &'a DB,
        key_or_prefix: &[u8],
        prefix: Option<Vec<u8>>,
        direction: SeekDirection,
    ) -> Self {
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
        Self {
            inner,
            prefix,
            done: false,
        }
    }

    fn matches_prefix(&self, key: &[u8]) -> bool {
        if let Some(prefix) = &self.prefix {
            key.starts_with(prefix)
        } else {
            true
        }
    }
}

impl<'a> Iterator for StoreIterator<'a> {
    type Item = (Vec<u8>, Vec<u8>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        while let Some(result) = self.inner.next() {
            match result {
                Ok((key, value)) => {
                    let key_vec = key.to_vec();
                    if self.matches_prefix(&key_vec) {
                        return Some((key_vec, value.to_vec()));
                    }

                    if self.prefix.is_some() {
                        self.done = true;
                        return None;
                    }
                }
                Err(_) => {
                    self.done = true;
                    return None;
                }
            }
        }

        None
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
        let prefix = key_prefix.cloned();
        let start_prefix = prefix.clone();
        let start = start_prefix.as_deref().unwrap_or(&[]);
        let iterator = self.iterator_from(start, prefix, direction);
        Box::new(iterator)
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for Store {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw = key.to_array();
        self.db.get(raw).ok().flatten().map(StorageItem::from_bytes)
    }

    fn contains(&self, key: &StorageKey) -> bool {
        let raw = key.to_array();
        self.db.get_pinned(raw).ok().flatten().is_some()
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let start = prefix_bytes.as_deref().unwrap_or(&[]);
        let iter = self.iterator_from(start, prefix_bytes.clone(), direction);
        Box::new(iter.filter_map(move |(key, value)| {
            if let Some(prefix) = &prefix_bytes {
                if !key.starts_with(prefix) {
                    return None;
                }
            }
            Some((StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
        }))
    }
}

impl IReadOnlyStore for Store {}

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

    fn as_any(&self) -> &dyn Any {
        self
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::persistence::seek_direction::SeekDirection;
    use neo_core::smart_contract::{StorageItem, StorageKey};
    use tempfile::TempDir;

    #[test]
    fn store_roundtrip_supports_storage_keys_and_iteration() {
        let tmp = TempDir::new().expect("temp dir");
        let mut store = Store::open(tmp.path());

        let key = StorageKey::new(7, vec![1, 2, 3]);
        let value = StorageItem::from_bytes(b"hello".to_vec());
        let raw_key = key.to_array();
        let raw_value = value.get_value();

        store.put(raw_key.clone(), raw_value.clone());

        let fetched = store.try_get(&key).expect("value present");
        assert_eq!(fetched.get_value(), raw_value);

        let items: Vec<_> = store.find(Some(&key), SeekDirection::Forward).collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, StorageKey::from_bytes(&raw_key));
        assert_eq!(items[0].1.get_value(), raw_value);

        // Byte-level snapshot access still works for read-only paths.
        let snapshot = store.get_snapshot();
        let snap_value = snapshot.try_get(&raw_key).expect("snapshot value");
        assert_eq!(snap_value, raw_value);
        let snap_items: Vec<_> = snapshot
            .find(Some(&raw_key), SeekDirection::Forward)
            .collect();
        assert_eq!(snap_items.len(), 1);
        assert_eq!(snap_items[0].0, raw_key);
        assert_eq!(snap_items[0].1, raw_value);
    }

    #[test]
    fn find_respects_prefix_bounds_for_bytes() {
        let tmp = TempDir::new().expect("temp dir");
        let mut store = Store::open(tmp.path());

        store.put(b"foo/1".to_vec(), b"a".to_vec());
        store.put(b"foo/2".to_vec(), b"b".to_vec());
        store.put(b"fzz".to_vec(), b"c".to_vec());

        let prefix = b"foo".to_vec();
        let items: Vec<_> = store.find(Some(&prefix), SeekDirection::Forward).collect();

        assert_eq!(
            items.len(),
            2,
            "keys outside the prefix must not be returned"
        );
        assert!(items.iter().all(|(k, _)| k.starts_with(&prefix)));

        let snapshot = store.get_snapshot();
        let snap_items: Vec<_> = snapshot
            .find(Some(&prefix), SeekDirection::Forward)
            .collect();
        assert_eq!(
            snap_items.len(),
            2,
            "snapshot iteration should obey the same prefix bounds"
        );
        assert!(snap_items.iter().all(|(k, _)| k.starts_with(&prefix)));
    }
}

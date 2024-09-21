use std::collections::{HashMap, HashSet};
use std::sync::MutexGuard;
use bytes::BufMut;
use crate::neo_contract::storage_item::StorageItem;
use crate::neo_contract::storage_key::StorageKey;
use crate::persistence::{DataCache, SeekDirection, SnapshotTrait, Trackable};
use crate::persistence::persistence_error::PersistenceError;
use crate::store::ReadOnlyStore;

/// Represents a cache for the snapshot or database of the NEO blockchain.
pub struct SnapshotCache {
    store: Box<dyn ReadOnlyStore>,
    snapshot: Option<Box<dyn SnapshotTrait>>,
    cache: HashMap<Vec<u8>, Option<Vec<u8>>>,
}

impl SnapshotCache {
    /// Initializes a new instance of the SnapshotCache struct.
    ///
    /// # Arguments
    ///
    /// * `store` - A type that implements ReadOnlyStore to create a readonly cache,
    ///             or a type that implements Snapshot to create a snapshot cache.
    pub fn new<T: ReadOnlyStore + 'static>(store: T) -> Self {
        let snapshot = store.as_any().downcast_ref::<dyn SnapshotTrait>().map(|s| Box::new(s.clone()) as Box<dyn SnapshotTrait>);
        SnapshotCache {
            store: Box::new(store),
            snapshot,
            cache: HashMap::new(),
        }
    }

    fn add_internal(&mut self, key: &[u8], value: &[u8]) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.put(key, value);
        }
        self.cache.insert(key.to_vec(), Some(value.to_vec()));
    }

    fn delete_internal(&mut self, key: &[u8]) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.delete(key);
        }
        self.cache.insert(key.to_vec(), None);
    }

    pub fn commit(&mut self) {
        for (key, value_opt) in self.cache.drain() {
            match value_opt {
                Some(value) => self.store.put(&key, &value),
                None => self.store.delete(&key),
            }
        }
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.commit();
        }
    }

    fn contains_internal(&self, key: &[u8]) -> bool {
        self.store.contains(key)
    }

    fn get_internal(&self, key: &[u8]) -> Result<Vec<u8>, Error> {
        self.store.try_get(key).ok_or_else(|| Error::KeyNotFound)
    }

    fn seek_internal(&self, key_or_prefix: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        Box::new(self.store.seek(key_or_prefix, direction))
    }

    fn try_get_internal(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.store.try_get(key)
    }

    fn update_internal(&mut self, key: &[u8], value: &[u8]) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.put(key, value);
        }
        self.cache.insert(key.to_vec(), Some(value.to_vec()));
    }
}

impl DataCache for SnapshotCache {
    fn add(&mut self, key: &[u8], value: &[u8]) {
        self.add_internal(key, value);
    }

    fn delete(&mut self, key: &[u8]) {
        self.delete_internal(key);
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.contains_internal(key)
    }

    fn get(&self, key: &[u8]) -> Result<Vec<u8>, Error> {
        self.get_internal(key)
    }

    fn seek(&self, key_or_prefix: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        self.seek_internal(key_or_prefix, direction)
    }

    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.try_get_internal(key)
    }

    fn update(&mut self, key: &[u8], value: &[u8]) {
        self.update_internal(key, value);
    }

    fn new() -> Self
    where
        Self: Sized
    {
        todo!()
    }

    fn get_internal(&self, key: &StorageKey) -> Result<StorageItem, PersistenceError> {
        todo!()
    }

    fn add_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError> {
        todo!()
    }

    fn delete_internal(&self, key: &StorageKey) -> Result<(), PersistenceError> {
        todo!()
    }

    fn contains_internal(&self, key: &StorageKey) -> bool {
        todo!()
    }

    fn try_get_internal(&self, key: &StorageKey) -> Result<Option<StorageItem>, &'static str> {
        todo!()
    }

    fn update_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError> {
        todo!()
    }

    fn seek_internal(&self, key_or_prefix: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item=(StorageKey, StorageItem)> + '_> {
        todo!()
    }

    fn get_dictionary(&self) -> MutexGuard<'_, HashMap<StorageKey, Trackable>> {
        todo!()
    }

    fn get_change_set(&self) -> MutexGuard<'_, HashSet<StorageKey>> {
        todo!()
    }

    fn get_change_set_iter(&self) -> Box<dyn Iterator<Item=Trackable> + '_> {
        todo!()
    }
}

impl Drop for SnapshotCache {
    fn drop(&mut self) {
        if let Some(snapshot) = &mut self.snapshot {
            snapshot.dispose();
        }
    }
}

use std::collections::{HashMap, HashSet};
use std::io::Seek;
use crate::neo_contract::storage_item::StorageItem;
use crate::neo_contract::storage_key::StorageKey;
use crate::persistence::{DataCache, SeekDirection, Trackable};
use crate::persistence::persistence_error::PersistenceError;

pub struct ClonedCache {
    inner_cache: Box<dyn DataCache>,
}

impl ClonedCache {
    pub fn new(inner_cache: Box<dyn DataCache>) -> Self {
        ClonedCache { inner_cache }
    }
}

impl DataCache for ClonedCache {
    fn new() -> Self where Self: Sized {
        unimplemented!("ClonedCache cannot be created without an inner cache")
    }

    fn get_internal(&self, key: &StorageKey) -> Result<StorageItem, PersistenceError>{
        self.inner_cache.get(key).map(|item| item.clone())
    }

    fn add_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError>{
        self.inner_cache.add(key.clone(), value.clone())
    }

    fn delete_internal(&self, key: &StorageKey) -> Result<(), PersistenceError>{
        self.inner_cache.delete(key)
    }

    fn contains_internal(&self, key: &StorageKey) -> bool {
        self.inner_cache.contains(key)
    }

    fn try_get_internal(&self, key: &StorageKey) -> Result<Option<StorageItem>, PersistenceError>{
        self.inner_cache.try_get(key).map(|opt| opt.map(|item| item.clone()))
    }

    fn update_internal(&self, key: &StorageKey, value: &StorageItem) -> Result<(), PersistenceError>{
        self.inner_cache.get_and_change(key, None)
            .and_then(|opt| opt.ok_or("Key not found"))
            .map(|mut item| {
                item.from_replica(value);
                Ok(())
            })?
    }

    fn seek_internal(&self, key_or_prefix: &[u8], direction: SeekDirection)
                     -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        Box::new(self.inner_cache.seek(Some(key_or_prefix), direction)
            .map(|(key, value)| (key, value.clone())))
    }

    fn get_dictionary(&self) -> std::sync::MutexGuard<'_, HashMap<StorageKey, Trackable>> {
        self.inner_cache.get_dictionary()
    }

    fn get_change_set(&self) -> std::sync::MutexGuard<'_, HashSet<StorageKey>> {
        self.inner_cache.get_change_set()
    }

    fn get_change_set_iter(&self) -> Box<dyn Iterator<Item = Trackable> + '_> {
        self.inner_cache.get_change_set_iter()
    }
}

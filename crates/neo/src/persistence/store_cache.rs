// Copyright (C) 2015-2025 The Neo Project.
//
// store_cache.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    data_cache::DataCache,
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    i_store::IStore,
    i_store_snapshot::IStoreSnapshot,
    seek_direction::SeekDirection,
};
use crate::smart_contract::{StorageItem, StorageKey};
use std::sync::Arc;

/// Represents a cache for the snapshot or database of the NEO blockchain.
pub struct StoreCache {
    data_cache: DataCache,
    store: Arc<dyn IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> + Send + Sync>,
    snapshot: Option<Arc<dyn IStoreSnapshot>>,
}

impl StoreCache {
    /// Initializes a new instance of the StoreCache class with a store.
    pub fn new_from_store(store: Arc<dyn IStore>, read_only: bool) -> Self {
        Self {
            data_cache: DataCache::new(read_only),
            store: store.clone() as Arc<dyn IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> + Send + Sync>,
            snapshot: None,
        }
    }

    /// Initializes a new instance of the StoreCache class with a snapshot.
    pub fn new_from_snapshot(snapshot: Arc<dyn IStoreSnapshot>) -> Self {
        Self {
            data_cache: DataCache::new(false),
            store: snapshot.clone()
                as Arc<dyn IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> + Send + Sync>,
            snapshot: Some(snapshot),
        }
    }

    /// Commits all changes.
    pub fn commit(&mut self) {
        self.data_cache.commit();
        if let Some(ref mut snapshot) = self.snapshot {
            // Need mutable reference to commit
            // In practice, this would be handled differently
            // snapshot.commit();
        }
    }

    /// Gets an item from the cache or underlying store.
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        // First check the cache
        if let Some(item) = self.data_cache.get(key) {
            return Some(item);
        }

        // Then check the underlying store
        let key_bytes = key.to_array();
        if let Some(value_bytes) = self.store.try_get(&key_bytes) {
            return Some(StorageItem::from_bytes(&value_bytes));
        }

        None
    }

    /// Adds an item to the cache.
    pub fn add(&mut self, key: StorageKey, value: StorageItem) {
        self.data_cache.add(key.clone(), value.clone());

        if let Some(ref mut snapshot) = self.snapshot {
            let key_bytes = key.to_array();
            let value_bytes = value.to_array();
            // Need mutable reference to put
            // snapshot.put(key_bytes, value_bytes);
        }
    }

    /// Updates an item in the cache.
    pub fn update(&mut self, key: StorageKey, value: StorageItem) {
        self.data_cache.update(key.clone(), value.clone());

        if let Some(ref mut snapshot) = self.snapshot {
            let key_bytes = key.to_array();
            let value_bytes = value.to_array();
            // Need mutable reference to put
            // snapshot.put(key_bytes, value_bytes);
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&mut self, key: StorageKey) {
        self.data_cache.delete(key.clone());

        if let Some(ref mut snapshot) = self.snapshot {
            let key_bytes = key.to_array();
            // Need mutable reference to delete
            // snapshot.delete(key_bytes);
        }
    }
}

impl IReadOnlyStore for StoreCache {}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for StoreCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        // First get items from cache
        let cache_items = self.data_cache.find(key_prefix, direction);

        // Then get items from store
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let store_items = self.store.find(prefix_bytes.as_ref(), direction);

        // Merge and deduplicate
        // This is simplified - in practice would need proper merging
        Box::new(cache_items.chain(
            store_items.map(|(k, v)| (StorageKey::from_bytes(&k), StorageItem::from_bytes(&v))),
        ))
    }
}

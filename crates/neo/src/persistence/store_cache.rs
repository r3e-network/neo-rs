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
    store: Option<Arc<dyn IStore>>,
    snapshot: Option<Arc<dyn IStoreSnapshot>>,
}

impl StoreCache {
    /// Initializes a new instance of the StoreCache class with a store.
    pub fn new_from_store(store: Arc<dyn IStore>, read_only: bool) -> Self {
        let store_for_get = store.clone();
        let store_for_find = store.clone();
        let store_get: Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync> =
            Arc::new(move |key: &StorageKey| store_for_get.try_get(key));
        let store_find: Arc<
            dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)>
                + Send
                + Sync,
        > = Arc::new(move |prefix, direction| {
            store_for_find
                .find(prefix, direction)
                .collect::<Vec<(StorageKey, StorageItem)>>()
        });
        Self {
            data_cache: DataCache::new_with_store(read_only, Some(store_get), Some(store_find)),
            store: Some(store),
            snapshot: None,
        }
    }

    /// Provides read-only access to the underlying in-memory data cache.
    pub fn data_cache(&self) -> &DataCache {
        &self.data_cache
    }

    /// Initializes a new instance of the StoreCache class with a snapshot.
    pub fn new_from_snapshot(snapshot: Arc<dyn IStoreSnapshot>) -> Self {
        Self {
            data_cache: DataCache::new(false),
            store: None,
            snapshot: Some(snapshot),
        }
    }

    /// Commits all changes.
    pub fn commit(&mut self) {
        self.data_cache.commit();
        if self.snapshot.is_some() {
            // Snapshot commit will be wired once mutability story is in place.
        }
    }

    /// Gets an item from the cache or underlying store.
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        // First check the cache
        if let Some(item) = self.data_cache.get(key) {
            return Some(item);
        }

        if let Some(store) = &self.store {
            if let Some(item) = store.try_get(key) {
                return Some(item);
            }
        }

        if let Some(snapshot) = &self.snapshot {
            let key_bytes = key.to_array();
            if let Some(value_bytes) = snapshot.try_get(&key_bytes) {
                return Some(StorageItem::from_bytes(value_bytes));
            }
        }

        None
    }

    /// Adds an item to the cache.
    pub fn add(&mut self, key: StorageKey, value: StorageItem) {
        self.data_cache.add(key.clone(), value.clone());

        if self.snapshot.is_some() {
            // Snapshot propagation pending implementation.
        }
    }

    /// Updates an item in the cache.
    pub fn update(&mut self, key: StorageKey, value: StorageItem) {
        self.data_cache.update(key.clone(), value.clone());

        if self.snapshot.is_some() {
            // Snapshot propagation pending implementation.
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&mut self, key: StorageKey) {
        self.data_cache.delete(&key);

        if self.snapshot.is_some() {
            // Snapshot propagation pending implementation.
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
        let cache_items = self.data_cache.find(key_prefix, direction);

        let store_items: Box<dyn Iterator<Item = (StorageKey, StorageItem)>> =
            if let Some(store) = &self.store {
                store.find(key_prefix, direction)
            } else {
                Box::new(std::iter::empty())
            };

        let snapshot_items: Box<dyn Iterator<Item = (StorageKey, StorageItem)>> =
            if let Some(snapshot) = &self.snapshot {
                let prefix_bytes = key_prefix.map(|k| k.to_array());
                Box::new(
                    snapshot
                        .find(prefix_bytes.as_ref(), direction)
                        .map(|(key, value)| {
                            (StorageKey::from_bytes(&key), StorageItem::from_bytes(value))
                        }),
                )
            } else {
                Box::new(std::iter::empty())
            };

        Box::new(cache_items.chain(store_items).chain(snapshot_items))
    }
}

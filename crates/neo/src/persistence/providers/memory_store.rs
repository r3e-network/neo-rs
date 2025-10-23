// Copyright (C) 2015-2025 The Neo Project.
//
// memory_store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::memory_snapshot::MemorySnapshot;
use crate::persistence::{
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    i_store::{IStore, OnNewSnapshotDelegate},
    i_store_snapshot::IStoreSnapshot,
    i_write_store::IWriteStore,
    seek_direction::SeekDirection,
};
use crate::smart_contract::{storage_key::StorageKey, StorageItem};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// An in-memory IStore implementation that uses BTreeMap as the underlying storage.
pub struct MemoryStore {
    inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
}

impl MemoryStore {
    /// Creates a new MemoryStore.
    pub fn new() -> Self {
        Self {
            inner_data: Arc::new(RwLock::new(BTreeMap::new())),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Resets the store, clearing all data.
    pub fn reset(&self) {
        self.inner_data.write().unwrap().clear();
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemoryStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner_data.read().unwrap().get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let data = self.inner_data.read().unwrap();
        let iter: Vec<_> = if let Some(prefix) = key_prefix {
            data.iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        } else {
            data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };

        let result = if direction == SeekDirection::Backward {
            Box::new(iter.into_iter().rev()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        } else {
            Box::new(iter.into_iter()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        };

        result
    }
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for MemoryStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw_key = key.to_array();
        self.inner_data
            .read()
            .unwrap()
            .get(&raw_key)
            .cloned()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let data = self.inner_data.read().unwrap();
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        let mut entries: Vec<_> = data
            .iter()
            .filter(|(key, _)| {
                if let Some(prefix) = prefix_bytes.as_ref() {
                    key.starts_with(prefix)
                } else {
                    true
                }
            })
            .map(|(key, value)| {
                (
                    StorageKey::from_bytes(key),
                    StorageItem::from_bytes(value.clone()),
                )
            })
            .collect();

        if direction == SeekDirection::Backward {
            entries.reverse();
        }

        Box::new(entries.into_iter())
    }
}

impl IWriteStore<Vec<u8>, Vec<u8>> for MemoryStore {
    fn delete(&mut self, key: Vec<u8>) {
        self.inner_data.write().unwrap().remove(&key);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.inner_data.write().unwrap().insert(key, value);
    }
}

impl IReadOnlyStore for MemoryStore {}

impl IStore for MemoryStore {
    fn get_snapshot(&self) -> Arc<dyn IStoreSnapshot> {
        let snapshot = Arc::new(MemorySnapshot::new(
            Arc::new(self.clone()),
            self.inner_data.clone(),
        ));

        // Trigger event
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

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self {
            inner_data: self.inner_data.clone(),
            on_new_snapshot: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl MemoryStore {
    /// Applies a batch of write operations to the underlying store.
    pub fn apply_batch(&self, batch: &std::collections::BTreeMap<Vec<u8>, Option<Vec<u8>>>) {
        let mut guard = self.inner_data.write().unwrap();
        for (key, value) in batch.iter() {
            match value {
                Some(v) => {
                    guard.insert(key.clone(), v.clone());
                }
                None => {
                    guard.remove(key);
                }
            }
        }
    }
}

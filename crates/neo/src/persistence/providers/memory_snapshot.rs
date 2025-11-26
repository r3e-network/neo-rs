//! In-memory snapshot implementation for persistence providers.

use super::memory_store::MemoryStore;
use crate::persistence::{
    i_read_only_store::IReadOnlyStoreGeneric, i_store::IStore, i_store_snapshot::IStoreSnapshot,
    i_write_store::IWriteStore, seek_direction::SeekDirection,
};
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// On-chain write operations on a snapshot cannot be concurrent.
type WriteBatch = Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>;

pub struct MemorySnapshot {
    store: Arc<dyn IStore>,
    immutable_data: BTreeMap<Vec<u8>, Vec<u8>>,
    write_batch: WriteBatch,
}

impl MemorySnapshot {
    /// Creates a new MemorySnapshot.
    pub fn new(
        store: Arc<dyn IStore>,
        inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    ) -> Self {
        let immutable_data = inner_data.read().unwrap().clone();
        Self {
            store,
            immutable_data,
            write_batch: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Gets the number of items in the write batch.
    pub fn write_batch_length(&self) -> usize {
        self.write_batch.read().unwrap().len()
    }
}

impl IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemorySnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        // Check write batch first
        if let Some(batch_value) = self.write_batch.read().unwrap().get(key) {
            return batch_value.clone();
        }
        // Then check immutable data
        self.immutable_data.get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        // Merge immutable data with write batch
        let mut merged = self.immutable_data.clone();

        // Apply write batch changes
        for (key, value) in self.write_batch.read().unwrap().iter() {
            if let Some(v) = value {
                merged.insert(key.clone(), v.clone());
            } else {
                merged.remove(key);
            }
        }

        let iter: Vec<_> = if let Some(prefix) = key_prefix {
            merged
                .into_iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .collect()
        } else {
            merged.into_iter().collect()
        };

        if direction == SeekDirection::Backward {
            Box::new(iter.into_iter().rev())
        } else {
            Box::new(iter.into_iter())
        }
    }
}

impl IWriteStore<Vec<u8>, Vec<u8>> for MemorySnapshot {
    fn delete(&mut self, key: Vec<u8>) {
        self.write_batch.write().unwrap().insert(key, None);
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.write_batch.write().unwrap().insert(key, Some(value));
    }
}

impl IStoreSnapshot for MemorySnapshot {
    fn store(&self) -> Arc<dyn IStore> {
        self.store.clone()
    }

    fn commit(&mut self) {
        {
            // Apply write batch to the store
            let batch = self.write_batch.read().unwrap();
            // If the underlying store is a MemoryStore, apply batch directly.
            if let Some(mem) = self.store.as_any().downcast_ref::<MemoryStore>() {
                mem.apply_batch(&batch);
            }
            // drop read guard before acquiring write lock
        }

        // Clear the write batch
        self.write_batch.write().unwrap().clear();
    }
}

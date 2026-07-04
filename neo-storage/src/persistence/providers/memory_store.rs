use super::memory_snapshot::MemorySnapshot;
use crate::persistence::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{OnNewSnapshotDelegate, Store},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::types::{StorageItem, storage_key::StorageKey};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// An in-memory Store implementation that uses BTreeMap as the underlying storage.
pub struct MemoryStore {
    inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    on_new_snapshot: Arc<RwLock<Vec<OnNewSnapshotDelegate>>>,
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore").finish_non_exhaustive()
    }
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
        self.inner_data.write().clear();
    }
}

neo_io::impl_default_via_new!(MemoryStore);

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemoryStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner_data.read().get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let data = self.inner_data.read();
        let iter: Vec<_> = data
            .iter()
            .filter(|(key, _)| {
                key_prefix
                    .map(|prefix| key.starts_with(prefix))
                    .unwrap_or(true)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if direction == SeekDirection::Backward {
            Box::new(iter.into_iter().rev()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        } else {
            Box::new(iter.into_iter()) as Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>
        }
    }
}

impl RawReadOnlyStore for MemoryStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner_data.read().get(key).cloned()
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for MemoryStore {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw_key = key.to_array();
        self.inner_data
            .read()
            .get(&raw_key)
            .cloned()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let data = self.inner_data.read();
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

impl WriteStore<Vec<u8>, Vec<u8>> for MemoryStore {
    fn delete(&mut self, key: Vec<u8>) -> crate::error::StorageResult<()> {
        self.inner_data.write().remove(&key);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::error::StorageResult<()> {
        self.inner_data.write().insert(key, value);
        Ok(())
    }
}

impl ReadOnlyStore for MemoryStore {}

impl Store for MemoryStore {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
        let snapshot = Arc::new(MemorySnapshot::new(
            Arc::new(self.clone()),
            self.inner_data.clone(),
        ));

        // Trigger event
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
        let mut guard = self.inner_data.write();
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

#[cfg(test)]
#[path = "../../tests/persistence/providers/memory_store.rs"]
mod tests;

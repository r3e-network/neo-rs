use super::memory_snapshot::MemorySnapshot;
use crate::persistence::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store::{RawOverlaySource, Store, StoreBackendKind},
    store_maintenance::StoreMaintenanceBatch,
    transactional_store::TransactionalStore,
    write_store::WriteStore,
};
use crate::types::{StorageItem, storage_key::StorageKey};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// An in-memory Store implementation that uses BTreeMap as the underlying storage.
pub struct MemoryStore {
    state: Arc<RwLock<MemoryStoreState>>,
}

#[derive(Default)]
pub(super) struct MemoryStoreState {
    pub(super) data: BTreeMap<Vec<u8>, Vec<u8>>,
    metadata: BTreeMap<Vec<u8>, Vec<u8>>,
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
            state: Arc::new(RwLock::new(MemoryStoreState::default())),
        }
    }

    /// Resets the store, clearing normal data and maintenance metadata.
    pub fn reset(&self) {
        *self.state.write() = MemoryStoreState::default();
    }

    fn commit_borrowed_overlay<O>(&self, overlay_source: &mut O) -> crate::error::StorageResult<()>
    where
        O: RawOverlaySource + ?Sized,
    {
        let mut state = self.state.write();
        let mut sink = |key: &[u8], value: Option<&[u8]>| match value {
            Some(value) => {
                state.data.insert(key.to_vec(), value.to_vec());
            }
            None => {
                state.data.remove(key);
            }
        };
        overlay_source.visit_raw_overlay(&mut sink);
        Ok(())
    }
}

neo_io::impl_default_via_new!(MemoryStore);

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemoryStore {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.state.read().data.get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let state = self.state.read();
        let mut entries: Vec<_> = state
            .data
            .iter()
            .filter(|(key, _)| {
                key_prefix
                    .map(|prefix| key.starts_with(prefix))
                    .unwrap_or(true)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        entries.into_iter()
    }
}

impl RawReadOnlyStore for MemoryStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.state.read().data.get(key).cloned()
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for MemoryStore {
    type FindIterator<'a> = std::vec::IntoIter<(StorageKey, StorageItem)>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        let raw_key = key.to_array();
        self.state
            .read()
            .data
            .get(&raw_key)
            .cloned()
            .map(StorageItem::from_bytes)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let state = self.state.read();
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        let mut entries: Vec<_> = state
            .data
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

        entries.into_iter()
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for MemoryStore {
    fn delete(&mut self, key: Vec<u8>) -> crate::error::StorageResult<()> {
        self.state.write().data.remove(&key);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::error::StorageResult<()> {
        self.state.write().data.insert(key, value);
        Ok(())
    }
}

impl ReadOnlyStore for MemoryStore {}

impl Store for MemoryStore {
    type Snapshot = MemorySnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        Arc::new(MemorySnapshot::new(
            Arc::new(self.clone()),
            self.state.clone(),
        ))
    }

    fn backend_kind(&self) -> StoreBackendKind {
        StoreBackendKind::Memory
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> crate::error::StorageResult<bool> {
        let mut state = self.state.write();
        for (key, value) in overlay {
            match value {
                Some(value) => {
                    state.data.insert(key.clone(), value.clone());
                }
                None => {
                    state.data.remove(key);
                }
            }
        }
        Ok(true)
    }

    fn try_commit_borrowed_raw_overlay<O>(
        &self,
        overlay_source: &mut O,
    ) -> crate::error::StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        self.commit_borrowed_overlay(overlay_source)?;
        Ok(true)
    }
}

impl TransactionalStore for MemoryStore {
    fn commit_canonical_overlay<O>(&self, overlay_source: &mut O) -> crate::error::StorageResult<()>
    where
        O: RawOverlaySource + ?Sized,
    {
        self.commit_borrowed_overlay(overlay_source)
    }

    fn maintenance_metadata(&self, key: &[u8]) -> crate::error::StorageResult<Option<Vec<u8>>> {
        Ok(self.state.read().metadata.get(key).cloned())
    }

    fn commit_maintenance(
        &self,
        maintenance: &StoreMaintenanceBatch,
    ) -> crate::error::StorageResult<()> {
        let mut state = self.state.write();
        for (key, value) in maintenance.data_operations() {
            match value {
                Some(value) => {
                    state.data.insert(key.to_vec(), value.to_vec());
                }
                None => {
                    state.data.remove(key);
                }
            }
        }
        for (key, value) in maintenance.metadata_operations() {
            match value {
                Some(value) => {
                    state.metadata.insert(key.to_vec(), value.to_vec());
                }
                None => {
                    state.metadata.remove(key);
                }
            }
        }
        Ok(())
    }
}

impl Clone for MemoryStore {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl MemoryStore {
    /// Applies a batch of write operations to the underlying store.
    pub fn apply_batch(&self, batch: &std::collections::BTreeMap<Vec<u8>, Option<Vec<u8>>>) {
        let mut state = self.state.write();
        for (key, value) in batch.iter() {
            match value {
                Some(v) => {
                    state.data.insert(key.clone(), v.clone());
                }
                None => {
                    state.data.remove(key);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/persistence/providers/memory_store.rs"]
mod tests;

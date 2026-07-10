use super::memory_store::MemoryStore;
use crate::persistence::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// On-chain write operations on a snapshot cannot be concurrent.
type WriteBatch = Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>;

/// Point-in-time snapshot over an in-memory store.
#[derive(Debug)]
pub struct MemorySnapshot {
    store: Arc<MemoryStore>,
    immutable_data: BTreeMap<Vec<u8>, Vec<u8>>,
    write_batch: WriteBatch,
}

impl MemorySnapshot {
    /// Creates a new MemorySnapshot.
    pub fn new(
        store: Arc<MemoryStore>,
        inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
    ) -> Self {
        let immutable_data = inner_data.read().clone();
        Self {
            store,
            immutable_data,
            write_batch: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Gets the number of items in the write batch.
    pub fn write_batch_length(&self) -> usize {
        self.write_batch.read().len()
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for MemorySnapshot {
    type FindIterator<'a> = std::vec::IntoIter<(Vec<u8>, Vec<u8>)>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.immutable_data.get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        let mut entries: Vec<_> = self
            .immutable_data
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

impl RawReadOnlyStore for MemorySnapshot {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.immutable_data.get(key).cloned()
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for MemorySnapshot {
    fn delete(&mut self, key: Vec<u8>) -> crate::error::StorageResult<()> {
        self.write_batch.write().insert(key, None);
        Ok(())
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> crate::error::StorageResult<()> {
        self.write_batch.write().insert(key, Some(value));
        Ok(())
    }
}

impl StoreSnapshot for MemorySnapshot {
    type Store = MemoryStore;

    fn store(&self) -> Arc<Self::Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        {
            // Apply write batch to the store.
            let batch = self.write_batch.read();
            self.store.apply_batch(&batch);
            // drop read guard before acquiring write lock
        }

        // Only clear the write batch after the batch was successfully applied.
        self.write_batch.write().clear();

        Ok(())
    }
}

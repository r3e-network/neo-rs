use super::memory_store::MemoryStore;
use crate::persistence::{
    read_only_store::ReadOnlyStoreGeneric, seek_direction::SeekDirection, store::Store,
    store_snapshot::StoreSnapshot, write_store::WriteStore,
};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// On-chain write operations on a snapshot cannot be concurrent.
type WriteBatch = Arc<RwLock<BTreeMap<Vec<u8>, Option<Vec<u8>>>>>;

pub struct MemorySnapshot {
    store: Arc<dyn Store>,
    immutable_data: BTreeMap<Vec<u8>, Vec<u8>>,
    write_batch: WriteBatch,
}

impl MemorySnapshot {
    /// Creates a new MemorySnapshot.
    pub fn new(store: Arc<dyn Store>, inner_data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>) -> Self {
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
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.immutable_data.get(key).cloned()
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let iter: Vec<_> = self
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
            Box::new(iter.into_iter().rev())
        } else {
            Box::new(iter.into_iter())
        }
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
    fn store(&self) -> Arc<dyn Store> {
        self.store.clone()
    }

    fn try_commit(&mut self) -> crate::persistence::store_snapshot::SnapshotCommitResult {
        {
            // Apply write batch to the store.
            let batch = self.write_batch.read();
            // The underlying store must be a MemoryStore; otherwise the pending
            // writes have nowhere to go. Fail loudly instead of silently
            // discarding the batch, which would cause undetected data loss.
            match self.store.as_any().downcast_ref::<MemoryStore>() {
                Some(mem) => mem.apply_batch(&batch),
                None => {
                    return Err(crate::error::StorageError::CommitFailed(
                        "MemorySnapshot::try_commit: underlying store is not a MemoryStore; \
                         pending writes were not applied"
                            .to_string(),
                    ));
                }
            }
            // drop read guard before acquiring write lock
        }

        // Only clear the write batch after the batch was successfully applied.
        self.write_batch.write().clear();

        Ok(())
    }
}

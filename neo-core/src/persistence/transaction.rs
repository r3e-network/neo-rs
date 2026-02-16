//! Thin transactional wrapper over `StoreCache` for explicit commit semantics.

use super::{StoreCache, TrackState, Trackable, data_cache::DataCacheResult};
use crate::persistence::{i_store::IStore, i_store_snapshot::IStoreSnapshot};
use crate::smart_contract::StorageKey;
use std::sync::Arc;

/// Represents a write transaction over a store or snapshot.
///
/// This wrapper makes commit behaviour explicit and prevents accidental
/// swallowing of commit failures in call sites that only need to persist a
/// scoped batch of changes.
pub struct StoreTransaction {
    cache: StoreCache,
}

impl StoreTransaction {
    /// Creates a transaction bound to the provided store.
    pub fn from_store(store: Arc<dyn IStore>, read_only: bool) -> Self {
        Self {
            cache: StoreCache::new_from_store(store, read_only),
        }
    }

    /// Creates a transaction backed by a snapshot.
    pub fn from_snapshot(snapshot: Arc<dyn IStoreSnapshot>) -> Self {
        Self {
            cache: StoreCache::new_from_snapshot(snapshot),
        }
    }

    /// Mutable access to the underlying cache for staged mutations.
    pub fn cache_mut(&mut self) -> &mut StoreCache {
        &mut self.cache
    }

    /// Immutable access to the underlying cache.
    pub fn cache(&self) -> &StoreCache {
        &self.cache
    }

    /// Commits the transaction, returning an error if the underlying cache is read-only
    /// or the commit could not be applied.
    pub fn commit(mut self) -> DataCacheResult {
        self.cache.try_commit()
    }
}

impl From<StoreTransaction> for StoreCache {
    fn from(tx: StoreTransaction) -> Self {
        tx.cache
    }
}

/// Helper to apply a tracked change set onto a transactional cache.
pub fn apply_tracked_items(cache: &mut StoreCache, tracked: Vec<(StorageKey, Trackable)>) {
    for (key, trackable) in tracked {
        match trackable.state {
            TrackState::Added => cache.add(key, trackable.item),
            TrackState::Changed => cache.update(key, trackable.item),
            TrackState::Deleted => cache.delete(key),
            TrackState::None | TrackState::NotFound => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::providers::memory_store::MemoryStore;
    use crate::smart_contract::StorageItem;

    #[test]
    fn commit_read_only_transaction_fails() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let tx = StoreTransaction::from_store(store, true);
        assert!(
            tx.commit().is_err(),
            "read-only transaction should fail commit"
        );
    }

    #[test]
    fn apply_tracked_items_persists_changes() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut tx = StoreTransaction::from_store(store.clone(), false);

        let key = StorageKey::new(1, b"tracked".to_vec());
        let value = StorageItem::from_bytes(vec![1, 2, 3]);
        let track = Trackable::new(value.clone(), TrackState::Added);

        apply_tracked_items(tx.cache_mut(), vec![(key.clone(), track)]);
        tx.commit().expect("commit should succeed");

        let persisted = store.try_get(&key).expect("value should be persisted");
        assert_eq!(persisted.get_value(), value.get_value());
    }
}

//! Cache facade that fronts an `IStore` or snapshot for smart-contract storage.

use super::{
    data_cache::{DataCache, DataCacheError, DataCacheResult},
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    i_store::IStore,
    i_store_snapshot::IStoreSnapshot,
    seek_direction::SeekDirection,
    track_state::TrackState,
};
use crate::smart_contract::{StorageItem, StorageKey};
use std::sync::Arc;
use tracing::warn;

/// Represents a cache for the snapshot or database of the NEO blockchain.
type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;
type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;

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
        let store_get: Arc<StoreGetFn> =
            Arc::new(move |key: &StorageKey| store_for_get.try_get(key));
        let store_find: Arc<StoreFindFn> = Arc::new(move |prefix, direction| {
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
        let snapshot_for_get = snapshot.clone();
        let snapshot_for_find = snapshot.clone();
        let snapshot_get: Arc<StoreGetFn> = Arc::new(move |key: &StorageKey| {
            let key_bytes = key.to_array();
            snapshot_for_get
                .try_get(&key_bytes)
                .map(StorageItem::from_bytes)
        });
        let snapshot_find: Arc<StoreFindFn> = Arc::new(move |prefix, direction| {
            let prefix_bytes = prefix.map(|key| key.to_array());
            snapshot_for_find
                .find(prefix_bytes.as_ref(), direction)
                .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
                .collect::<Vec<(StorageKey, StorageItem)>>()
        });
        Self {
            data_cache: DataCache::new_with_store(false, Some(snapshot_get), Some(snapshot_find)),
            store: None,
            snapshot: Some(snapshot),
        }
    }

    /// Commits all changes.
    pub fn commit(&mut self) {
        if let Err(err) = self.try_commit() {
            warn!(target: "neo", error = ?err, "store cache commit failed");
        }
    }

    /// Commits all changes, returning an error if read-only.
    pub fn try_commit(&mut self) -> DataCacheResult {
        if self.data_cache.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }

        let tracked = self.data_cache.tracked_items();
        if tracked.is_empty() {
            self.data_cache.commit();
            return Ok(());
        }

        let mut writer_snapshot = if let Some(snapshot_arc) = self.snapshot.as_ref() {
            snapshot_arc.store().get_snapshot()
        } else if let Some(store_arc) = self.store.as_ref() {
            store_arc.get_snapshot()
        } else {
            let msg = "no backing store available for commit";
            warn!(target: "neo", "{msg}");
            return Err(DataCacheError::CommitFailed(msg.to_string()));
        };

        if let Some(snapshot) = Arc::get_mut(&mut writer_snapshot) {
            apply_tracked(&tracked, snapshot);
            snapshot.commit();
            self.data_cache.commit();
        } else {
            let msg = "unable to obtain mutable snapshot for commit; changes not persisted";
            warn!(target: "neo", "{msg}");
            return Err(DataCacheError::CommitFailed(msg.to_string()));
        }
        Ok(())
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
        let _ = self.try_add(key, value);
    }

    /// Adds an item to the cache, returning an error if the cache is read-only.
    ///
    /// Note: Changes are accumulated in the data cache and only propagated to the
    /// underlying snapshot/store during `commit()`. This matches C# SnapshotCache behavior.
    pub fn try_add(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_add(key, value)
    }

    /// Updates an item in the cache.
    pub fn update(&mut self, key: StorageKey, value: StorageItem) {
        let _ = self.try_update(key, value);
    }

    /// Updates an item in the cache, returning an error if the cache is read-only.
    ///
    /// Note: Changes are accumulated in the data cache and only propagated to the
    /// underlying snapshot/store during `commit()`. This matches C# SnapshotCache behavior.
    pub fn try_update(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_update(key, value)
    }

    /// Deletes an item from the cache.
    pub fn delete(&mut self, key: StorageKey) {
        let _ = self.try_delete(key);
    }

    /// Deletes an item from the cache, returning an error if the cache is read-only.
    ///
    /// Note: Changes are accumulated in the data cache and only propagated to the
    /// underlying snapshot/store during `commit()`. This matches C# SnapshotCache behavior.
    pub fn try_delete(&mut self, key: StorageKey) -> DataCacheResult {
        self.data_cache.try_delete(&key)
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::providers::memory_store::MemoryStore;

    #[test]
    fn read_only_store_cache_rejects_commit() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut cache = StoreCache::new_from_store(store, true);
        cache.add(
            StorageKey::new(1, b"a".to_vec()),
            StorageItem::from_bytes(vec![1]),
        );
        let result = cache.try_commit();
        assert_eq!(result, Err(DataCacheError::ReadOnly));
    }

    #[test]
    fn commit_without_backing_store_returns_error() {
        // Construct a cache with neither store nor snapshot by bypassing constructors.
        let mut cache = StoreCache {
            data_cache: DataCache::new(false),
            store: None,
            snapshot: None,
        };
        cache.add(
            StorageKey::new(9, b"missing".to_vec()),
            StorageItem::from_bytes(vec![1]),
        );
        let result = cache.try_commit();
        assert!(matches!(result, Err(DataCacheError::CommitFailed(_))));
    }

    #[test]
    fn read_only_store_cache_rejects_mutations() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut cache = StoreCache::new_from_store(store, true);
        let key = StorageKey::new(2, b"ro".to_vec());
        let item = StorageItem::from_bytes(vec![9]);

        assert_eq!(
            cache.try_add(key.clone(), item.clone()),
            Err(DataCacheError::ReadOnly)
        );
        assert_eq!(
            cache.try_update(key.clone(), item.clone()),
            Err(DataCacheError::ReadOnly)
        );
        assert_eq!(cache.try_delete(key.clone()), Err(DataCacheError::ReadOnly));
    }

    #[test]
    fn find_merges_snapshot_and_cache_entries() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        let key = StorageKey::new(1, b"suffix".to_vec());
        let cached_value = StorageItem::from_bytes(vec![1]);
        cache.add(key.clone(), cached_value.clone());

        // Persist a different key into the underlying store.
        let mut snapshot = store.get_snapshot();
        if let Some(snap) = Arc::get_mut(&mut snapshot) {
            snap.put(StorageKey::new(1, b"other".to_vec()).to_array(), vec![2]);
            snap.commit();
        }

        let mut entries: Vec<_> = cache
            .find(None, SeekDirection::Forward)
            .map(|(k, v)| (k.to_array(), v.get_value()))
            .collect();
        entries.sort();
        entries.dedup();

        let expected_a = (key.to_array(), cached_value.get_value());
        let expected_b = (StorageKey::new(1, b"other".to_vec()).to_array(), vec![2]);
        assert!(
            entries.contains(&expected_a) && entries.contains(&expected_b),
            "entries should contain both cache and store values: {:?}",
            entries
        );
    }

    #[test]
    fn commit_applies_tracked_items_to_store() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        let key = StorageKey::new(3, b"commit".to_vec());
        let value = StorageItem::from_bytes(vec![7, 7]);

        cache.add(key.clone(), value.clone());
        cache.commit();

        let persisted = store.try_get(&key).expect("persisted value");
        assert_eq!(persisted.get_value(), value.get_value());
    }

    #[test]
    fn snapshot_commit_persists_to_underlying_store() {
        let store: Arc<dyn IStore> = Arc::new(MemoryStore::new());
        let snapshot = store.get_snapshot();
        let mut cache = StoreCache::new_from_snapshot(snapshot);

        let key = StorageKey::new(4, b"snap".to_vec());
        let value = StorageItem::from_bytes(vec![3, 1, 4]);
        cache.add(key.clone(), value.clone());

        cache.commit();

        let persisted = store.try_get(&key).expect("persisted via snapshot commit");
        assert_eq!(persisted.get_value(), value.get_value());
    }
}

pub fn apply_tracked<T>(tracked: &[(StorageKey, super::data_cache::Trackable)], writer: &mut T)
where
    T: super::i_write_store::IWriteStore<Vec<u8>, Vec<u8>> + ?Sized,
{
    for (key, trackable) in tracked {
        match trackable.state {
            TrackState::Added | TrackState::Changed => {
                writer.put(key.to_array(), trackable.item.get_value());
            }
            TrackState::Deleted => writer.delete(key.to_array()),
            TrackState::None | TrackState::NotFound => {}
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

        if self.store.is_some() && self.snapshot.is_none() {
            // `data_cache.find` already merges the backing store and cache and sorts results.
            return Box::new(cache_items.into_iter());
        }

        let snapshot_items: Vec<(StorageKey, StorageItem)> = if let Some(snapshot) = &self.snapshot {
            let prefix_bytes = key_prefix.map(|k| k.to_array());
            snapshot
                .find(prefix_bytes.as_ref(), direction)
                .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
                .collect()
        } else {
            Vec::new()
        };

        let mut merged = std::collections::HashMap::new();
        for (key, value) in snapshot_items {
            merged.insert(key, value);
        }
        for (key, value) in cache_items {
            merged.insert(key, value);
        }

        let mut sorted: Vec<_> = merged.into_iter().collect();
        match direction {
            SeekDirection::Forward => sorted.sort_by(|a, b| a.0.cmp(&b.0)),
            SeekDirection::Backward => sorted.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        Box::new(sorted.into_iter())
    }
}

//! Cache facade that fronts a `Store` or snapshot for smart-contract storage.

use super::{
    data_cache::{DataCache, DataCacheConfig, DataCacheError, DataCacheResult},
    read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric},
    store::Store,
    store_snapshot::StoreSnapshot,
    seek_direction::SeekDirection,
    track_state::TrackState,
};
use crate::types::{StorageItem, StorageKey};
use crate::error::StorageResult;
use std::sync::Arc;
use tracing::warn;

/// Represents a cache for the snapshot or database of the NEO blockchain.
type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;
type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;

pub struct StoreCache {
    data_cache: DataCache,
    store: Option<Arc<dyn Store>>,
    snapshot: Option<Arc<dyn StoreSnapshot>>,
}

impl StoreCache {
    /// Initializes a new instance of the StoreCache class with a store.
    pub fn new_from_store(store: Arc<dyn Store>, read_only: bool) -> Self {
        Self::new_from_store_with_config(store, read_only, DataCacheConfig::default())
    }

    /// Initializes a new instance with a store and custom configuration.
    pub fn new_from_store_with_config(
        store: Arc<dyn Store>,
        read_only: bool,
        config: DataCacheConfig,
    ) -> Self {
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
            data_cache: DataCache::new_with_config(
                read_only,
                Some(store_get),
                Some(store_find),
                config,
            ),
            store: Some(store),
            snapshot: None,
        }
    }

    /// Provides read-only access to the underlying in-memory data cache.
    pub fn data_cache(&self) -> &DataCache {
        &self.data_cache
    }

    /// Initializes a new instance of the StoreCache class with a snapshot.
    pub fn new_from_snapshot(snapshot: Arc<dyn StoreSnapshot>) -> Self {
        Self::new_from_snapshot_with_config(snapshot, DataCacheConfig::default())
    }

    /// Initializes a new instance with a snapshot and custom cache configuration.
    pub fn new_from_snapshot_with_config(
        snapshot: Arc<dyn StoreSnapshot>,
        config: DataCacheConfig,
    ) -> Self {
        let snapshot_for_get = snapshot.clone();
        let snapshot_for_find = snapshot.clone();
        let snapshot_get: Arc<StoreGetFn> = Arc::new(move |key: &StorageKey| {
            let key_bytes = key.to_array();
            snapshot_for_get
                .try_get(&key_bytes)
                .map(StorageItem::from_bytes)
        });
        let snapshot_find: Arc<StoreFindFn> = Arc::new(move |prefix, direction| {
            let prefix_bytes = prefix.map(|key| key.as_bytes().into_owned());
            snapshot_for_find
                .find(prefix_bytes.as_ref(), direction)
                .map(|(key, value)| (StorageKey::from_bytes(&key), StorageItem::from_bytes(value)))
                .collect::<Vec<(StorageKey, StorageItem)>>()
        });
        Self {
            data_cache: DataCache::new_with_config(
                false,
                Some(snapshot_get),
                Some(snapshot_find),
                config,
            ),
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
            snapshot_arc.store().snapshot()
        } else if let Some(store_arc) = self.store.as_ref() {
            store_arc.snapshot()
        } else {
            let msg = "no backing store available for commit";
            warn!(target: "neo", "{msg}");
            return Err(DataCacheError::CommitFailed(msg.to_string()));
        };

        if let Some(snapshot) = Arc::get_mut(&mut writer_snapshot) {
            apply_tracked(&tracked, snapshot).map_err(|e| {
                DataCacheError::CommitFailed(format!("storage write failed: {}", e))
            })?;
            snapshot
                .try_commit()
                .map_err(|e| DataCacheError::CommitFailed(e.to_string()))?;
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
        self.data_cache.get(key)
    }

    /// Adds an item to the cache.
    pub fn add(&mut self, key: StorageKey, value: StorageItem) {
        let _ = self.try_add(key, value);
    }

    /// Adds an item to the cache, returning an error if the cache is read-only.
    pub fn try_add(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_add(key, value)
    }

    /// Updates an item in the cache.
    pub fn update(&mut self, key: StorageKey, value: StorageItem) {
        let _ = self.try_update(key, value);
    }

    /// Updates an item in the cache, returning an error if the cache is read-only.
    pub fn try_update(&mut self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.data_cache.try_update(key, value)
    }

    /// Deletes an item from the cache.
    pub fn delete(&mut self, key: StorageKey) {
        let _ = self.try_delete(key);
    }

    /// Deletes an item from the cache, returning an error if the cache is read-only.
    pub fn try_delete(&mut self, key: StorageKey) -> DataCacheResult {
        self.data_cache.try_delete(&key)
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.data_cache.find(key_prefix, direction)
    }
}

pub fn apply_tracked<T>(
    tracked: &[(StorageKey, super::data_cache::Trackable)],
    writer: &mut T,
) -> StorageResult<()>
where
    T: super::write_store::WriteStore<Vec<u8>, Vec<u8>> + ?Sized,
{
    for (key, trackable) in tracked {
        match trackable.state {
            TrackState::Added | TrackState::Changed => {
                writer.put(key.to_array(), trackable.item.to_value())?;
            }
            TrackState::Deleted => writer.delete(key.to_array())?,
            TrackState::None | TrackState::NotFound => {}
        }
    }
    Ok(())
}

impl ReadOnlyStore for StoreCache {}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for StoreCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.data_cache.find(key_prefix, direction)
    }
}

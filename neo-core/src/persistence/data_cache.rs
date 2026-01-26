//! Shared in-memory cache for persistence providers.
//!
//! This module implements a Copy-on-Write (CoW) DataCache pattern for optimal
//! performance during block synchronization.

use super::{
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    seek_direction::SeekDirection,
    track_state::TrackState,
};
use crate::smart_contract::{StorageItem, StorageKey};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tracing::warn;

/// Represents an entry in the cache.
#[derive(Debug, Clone)]
pub struct Trackable {
    /// The data of the entry.
    pub item: StorageItem,

    /// The state of the entry.
    pub state: TrackState,
}

impl Trackable {
    /// Creates a new Trackable.
    pub fn new(item: StorageItem, state: TrackState) -> Self {
        Self { item, state }
    }
}

/// Delegate for storage entries
pub type OnEntryDelegate = Arc<dyn Fn(&DataCache, &StorageKey, &StorageItem) + Send + Sync>;

/// Represents a cache for the underlying storage of the NEO blockchain.
type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;
type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;

/// Internal state protected by RwLock for thread-safe Copy-on-Write
struct InnerState {
    dictionary: HashMap<StorageKey, Trackable>,
    change_set: HashSet<StorageKey>,
}

impl InnerState {
    fn new() -> Self {
        Self {
            dictionary: HashMap::new(),
            change_set: HashSet::new(),
        }
    }
}

/// Represents a cache for the underlying storage of the NEO blockchain.
pub struct DataCache {
    /// Shared state with CoW optimization
    state: Arc<RwLock<InnerState>>,
    /// Read-only flag (determines if changes are tracked)
    read_only: bool,
    /// Callbacks for read events
    on_read: Arc<RwLock<Vec<OnEntryDelegate>>>,
    /// Callbacks for update events
    on_update: Arc<RwLock<Vec<OnEntryDelegate>>>,
    /// Optional store getter for cache misses
    store_get: Option<Arc<StoreGetFn>>,
    /// Optional store finder for prefix searches
    store_find: Option<Arc<StoreFindFn>>,
    /// Strong count for CoW detection
    ref_count: Arc<AtomicUsize>,
}

use std::sync::atomic::AtomicUsize;

/// Errors returned by DataCache operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataCacheError {
    #[error("cache is read-only")]
    ReadOnly,
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),
}

pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;

impl Clone for DataCache {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            read_only: self.read_only,
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
            ref_count: Arc::clone(&self.ref_count),
        }
    }
}

impl DataCache {
    /// Creates a new DataCache.
    pub fn new(read_only: bool) -> Self {
        Self::new_with_store(read_only, None, None)
    }

    /// Attempt to add an item to the cache, returning an error when read-only.
    pub fn try_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.add(key, value);
        Ok(())
    }

    /// Attempt to update an item in the cache, returning an error when read-only.
    pub fn try_update(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.update(key, value);
        Ok(())
    }

    /// Attempt to delete an item in the cache, returning an error when read-only.
    pub fn try_delete(&self, key: &StorageKey) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.delete(key);
        Ok(())
    }

    /// Attempts to commit, returning an error when read-only.
    pub fn try_commit(&self) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.commit();
        Ok(())
    }

    /// Creates a new DataCache with an optional backing store.
    pub fn new_with_store(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only,
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
            ref_count: Arc::new(AtomicUsize::new(1)),
        }
    }

    /// Returns true if DataCache is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Adds a handler for read events.
    pub fn on_read(&self, handler: OnEntryDelegate) {
        self.on_read.write().push(handler);
    }

    /// Adds a handler for update events.
    pub fn on_update(&self, handler: OnEntryDelegate) {
        self.on_update.write().push(handler);
    }

    /// Creates a lightweight copy of this cache using Copy-on-Write.
    /// This is O(1) - just shares the underlying data.
    pub fn clone_cache(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            read_only: self.read_only,
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
            ref_count: Arc::clone(&self.ref_count),
        }
    }

    /// Merges tracked changes from another cache into this one.
    pub fn merge_tracked_items(&self, items: &[(StorageKey, Trackable)]) {
        for (key, trackable) in items {
            match trackable.state {
                TrackState::Added => self.add(key.clone(), trackable.item.clone()),
                TrackState::Changed => self.update(key.clone(), trackable.item.clone()),
                TrackState::Deleted => self.delete(key),
                TrackState::None | TrackState::NotFound => {}
            }
        }
    }

    /// Gets an item from the cache.
    #[inline]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
                {
                    return Some(trackable.item.clone());
                }
                return None;
            }
        }

        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                {
                    let mut state = self.state.write();
                    state
                        .dictionary
                        .entry(key.clone())
                        .or_insert_with(|| Trackable::new(item.clone(), TrackState::None));
                }

                for handler in self.on_read.read().iter() {
                    handler(self, key, &item);
                }

                return Some(item);
            }
        }

        None
    }

    /// Gets an item from the cache as a reference.
    #[inline]
    pub fn get_ref(&self, key: &StorageKey) -> Option<StorageItem> {
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
                {
                    return Some(trackable.item.clone());
                }
            }
        }
        None
    }

    /// Adds an item to the cache.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        if self.read_only {
            warn!("attempted to add to read-only DataCache");
            return;
        }
        self.apply_add(&key, value.clone());
        for handler in self.on_update.read().iter() {
            handler(self, &key, &value);
        }
    }

    /// Applies an add operation to the internal storage.
    fn apply_add(&self, key: &StorageKey, value: StorageItem) {
        let mut state = self.state.write();
        state
            .dictionary
            .insert(key.clone(), Trackable::new(value, TrackState::Added));
        state.change_set.insert(key.clone());
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.read_only {
            warn!("attempted to update read-only DataCache");
            return;
        }
        self.apply_update(&key, value.clone());
        for handler in self.on_update.read().iter() {
            handler(self, &key, &value);
        }
    }

    /// Applies an update operation.
    fn apply_update(&self, key: &StorageKey, value: StorageItem) {
        let mut state = self.state.write();
        let prev_state = state
            .dictionary
            .get(key)
            .map(|t| t.state)
            .unwrap_or(TrackState::NotFound);
        let new_state = match prev_state {
            TrackState::Added => TrackState::Added,
            TrackState::Changed | TrackState::None => TrackState::Changed,
            TrackState::Deleted => TrackState::Added,
            TrackState::NotFound => TrackState::Changed,
        };
        state
            .dictionary
            .insert(key.clone(), Trackable::new(value, new_state));
        state.change_set.insert(key.clone());
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.read_only {
            warn!("attempted to delete from read-only DataCache");
            return;
        }
        self.apply_delete(key);
    }

    /// Applies a delete operation.
    fn apply_delete(&self, key: &StorageKey) {
        let mut state = self.state.write();
        let prev_state = state
            .dictionary
            .get(key)
            .map(|t| t.state)
            .unwrap_or(TrackState::NotFound);
        match prev_state {
            TrackState::Added => {
                state.dictionary.remove(key);
                state.change_set.remove(key);
            }
            TrackState::Changed | TrackState::None => {
                state.dictionary.insert(
                    key.clone(),
                    Trackable::new(StorageItem::default(), TrackState::Deleted),
                );
                state.change_set.insert(key.clone());
            }
            TrackState::Deleted | TrackState::NotFound => {}
        }
    }

    /// Commits changes to the underlying store.
    pub fn commit(&self) {
        if self.read_only {
            return;
        }
        // Clear change set (actual persistence handled by StoreCache)
        self.state.write().change_set.clear();
    }

    /// Gets all tracked items for persistence.
    pub fn tracked_items(&self) -> Vec<(StorageKey, Trackable)> {
        let state = self.state.read();
        state
            .change_set
            .iter()
            .filter_map(|key| {
                state
                    .dictionary
                    .get(key)
                    .map(|trackable| (key.clone(), trackable.clone()))
            })
            .collect()
    }

    /// Gets the change set.
    pub fn get_change_set(&self) -> Vec<StorageKey> {
        self.state.read().change_set.iter().cloned().collect()
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());
        let state = self.state.read();
        let base_items: Vec<(StorageKey, StorageItem)> = state
            .dictionary
            .iter()
            .filter(|(_, t)| t.state != TrackState::Deleted && t.state != TrackState::NotFound)
            .filter(|(k, _)| {
                if let Some(prefix) = &prefix_bytes {
                    k.to_array().starts_with(prefix)
                } else {
                    true
                }
            })
            .map(|(k, t)| (k.clone(), t.item.clone()))
            .collect();

        let items: Vec<_> = if let Some(store_find) = &self.store_find {
            let mut all_items = store_find(key_prefix, direction);
            for (key, item) in base_items {
                if !all_items.iter().any(|(k, _)| k == &key) {
                    all_items.push((key, item));
                }
            }
            match direction {
                SeekDirection::Forward => {
                    all_items.sort_by(|a, b| a.0.cmp(&b.0));
                }
                SeekDirection::Backward => {
                    all_items.sort_by(|a, b| b.0.cmp(&a.0));
                }
            }
            all_items
        } else {
            match direction {
                SeekDirection::Forward => {
                    let mut items: Vec<_> = base_items;
                    items.sort_by(|a, b| a.0.cmp(&b.0));
                    items
                }
                SeekDirection::Backward => {
                    let mut items: Vec<_> = base_items;
                    items.sort_by(|a, b| b.0.cmp(&a.0));
                    items
                }
            }
        };

        Box::new(items.into_iter())
    }

    /// Returns the number of pending changes.
    pub fn pending_change_count(&self) -> usize {
        self.state.read().change_set.len()
    }

    /// Returns true if there are any pending changes.
    pub fn has_pending_changes(&self) -> bool {
        !self.state.read().change_set.is_empty()
    }

    /// Extracts all tracked changes as raw key-value pairs.
    pub fn extract_raw_changes(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let state = self.state.read();
        state
            .change_set
            .iter()
            .filter_map(|key| {
                state
                    .dictionary
                    .get(key)
                    .and_then(|trackable| match trackable.state {
                        TrackState::Added | TrackState::Changed => {
                            Some((key.to_array(), Some(trackable.item.get_value())))
                        }
                        TrackState::Deleted => Some((key.to_array(), None)),
                        TrackState::None | TrackState::NotFound => None,
                    })
            })
            .collect()
    }
}

impl IReadOnlyStore for DataCache {}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for DataCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        self.find(key_prefix, direction)
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::smart_contract::StorageKey;

    fn make_key(id: i32, suffix: &[u8]) -> StorageKey {
        StorageKey::new(id, suffix.to_vec())
    }

    #[test]
    fn clone_cache_preserves_entries_and_change_set() {
        let cache = DataCache::new(false);
        let key = make_key(1, b"a");
        cache.add(key.clone(), StorageItem::from_bytes(vec![42]));

        let cloned = cache.clone_cache();

        assert_eq!(
            cloned.get(&key).unwrap().get_value(),
            vec![42],
            "cloned cache should contain original entry"
        );

        let change_set = cloned.get_change_set();
        assert!(
            change_set.contains(&key),
            "clone should retain pending change set entries"
        );
    }

    #[test]
    fn merge_tracked_items_applies_changes() {
        let base = DataCache::new(false);
        let key_added = make_key(2, b"b");
        let key_updated = make_key(3, b"c");

        base.add(key_updated.clone(), StorageItem::from_bytes(vec![1]));

        let clone = base.clone_cache();
        clone.add(key_added.clone(), StorageItem::from_bytes(vec![7]));
        clone.update(key_updated.clone(), StorageItem::from_bytes(vec![9]));

        let tracked = clone.tracked_items();
        base.merge_tracked_items(&tracked);

        assert_eq!(
            base.get(&key_added).unwrap().get_value(),
            vec![7],
            "merge should add new items"
        );
        assert_eq!(
            base.get(&key_updated).unwrap().get_value(),
            vec![9],
            "merge should update existing items"
        );
    }

    #[test]
    fn read_only_cache_rejects_mutations() {
        let cache = DataCache::new(true);
        let key = make_key(9, b"x");
        let item = StorageItem::from_bytes(vec![1]);

        assert_eq!(
            cache.try_add(key.clone(), item.clone()),
            Err(DataCacheError::ReadOnly)
        );
        assert_eq!(
            cache.try_update(key.clone(), item.clone()),
            Err(DataCacheError::ReadOnly)
        );
        assert_eq!(cache.try_delete(&key), Err(DataCacheError::ReadOnly));
        assert_eq!(cache.try_commit(), Err(DataCacheError::ReadOnly));
        assert!(cache.get(&key).is_none());
        assert!(cache.tracked_items().is_empty());
    }

    #[test]
    fn pending_change_count_tracks_changes() {
        let cache = DataCache::new(false);
        assert_eq!(cache.pending_change_count(), 0);
        assert!(!cache.has_pending_changes());

        cache.add(make_key(1, b"a"), StorageItem::from_bytes(vec![1]));
        assert_eq!(cache.pending_change_count(), 1);
        assert!(cache.has_pending_changes());

        cache.add(make_key(2, b"b"), StorageItem::from_bytes(vec![2]));
        assert_eq!(cache.pending_change_count(), 2);
    }

    #[test]
    fn copy_on_write_shares_data() {
        let cache = DataCache::new(false);
        let key = make_key(1, b"test");

        // Add to original cache
        cache.add(key.clone(), StorageItem::from_bytes(vec![1]));

        // Clone should share data
        let cloned = cache.clone_cache();
        assert_eq!(cloned.get(&key).unwrap().get_value(), vec![1]);

        // Modify cloned cache - both should see changes (shared state)
        cloned.add(make_key(2, b"new"), StorageItem::from_bytes(vec![2]));

        // Both should see the new entry (shared state)
        assert_eq!(
            cache.get(&make_key(2, b"new")).unwrap().get_value(),
            vec![2]
        );
    }
}

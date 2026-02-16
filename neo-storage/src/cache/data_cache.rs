//! In-memory cache for blockchain storage.
//!
//! This module provides the core `DataCache` implementation for
//! efficient storage operations with change tracking.

use super::trackable::Trackable;
use crate::types::{SeekDirection, StorageItem, StorageKey, TrackState};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use thiserror::Error;

/// Errors returned by `DataCache` operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DataCacheError {
    /// Cache is read-only and cannot be modified.
    #[error("cache is read-only")]
    ReadOnly,

    /// Unable to commit changes to the underlying store.
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),

    /// Key not found in cache.
    #[error("key not found")]
    KeyNotFound,
}

/// Result type for `DataCache` operations.
pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;

/// Function type for store get operations.
pub type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;

/// Function type for store find operations.
pub type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;

/// In-memory cache for blockchain storage with change tracking.
///
/// Provides efficient read/write access to storage items with:
/// - State tracking for batch commits
/// - Optional backing store delegation
/// - Read-only mode support
/// - Concurrent access via `RwLock`
///
/// # Example
///
/// ```rust,ignore
/// use neo_storage::cache::DataCache;
/// use neo_storage::types::{StorageKey, StorageItem};
///
/// let cache = DataCache::new(false);
/// let key = StorageKey::new(-1, vec![0x01]);
/// cache.add(key.clone(), StorageItem::new(vec![0xAA]));
/// assert!(cache.contains(&key));
/// ```
pub struct DataCache {
    /// In-memory dictionary of tracked entries.
    dictionary: Arc<RwLock<HashMap<StorageKey, Trackable>>>,
    /// Set of keys that have been modified (for commit optimization).
    change_set: Option<Arc<RwLock<HashSet<StorageKey>>>>,
    /// Whether this cache is read-only.
    read_only: bool,
    /// Optional function to fetch from backing store.
    store_get: Option<Arc<StoreGetFn>>,
    /// Optional function to find in backing store.
    store_find: Option<Arc<StoreFindFn>>,
}

impl Clone for DataCache {
    fn clone(&self) -> Self {
        Self {
            dictionary: Arc::new(RwLock::new(self.dictionary.read().clone())),
            change_set: self
                .change_set
                .as_ref()
                .map(|cs| Arc::new(RwLock::new(cs.read().clone()))),
            read_only: self.read_only,
            store_get: self.store_get.clone(),
            store_find: self.store_find.clone(),
        }
    }
}

impl DataCache {
    /// Creates a new empty cache.
    ///
    /// # Arguments
    ///
    /// * `read_only` - If true, the cache will reject write operations.
    #[must_use]
    pub fn new(read_only: bool) -> Self {
        Self::new_with_store(read_only, None, None)
    }

    /// Creates a cache with backing store functions.
    ///
    /// # Arguments
    ///
    /// * `read_only` - If true, the cache will reject write operations.
    /// * `store_get` - Optional function to fetch from backing store.
    /// * `store_find` - Optional function to search backing store.
    #[must_use]
    pub fn new_with_store(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
    ) -> Self {
        Self {
            dictionary: Arc::new(RwLock::new(HashMap::new())),
            change_set: if read_only {
                None
            } else {
                Some(Arc::new(RwLock::new(HashSet::new())))
            },
            read_only,
            store_get,
            store_find,
        }
    }

    /// Returns whether this cache is read-only.
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Gets an item from the cache or backing store.
    #[must_use]
    pub fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        // First check in-memory cache
        {
            let dict = self.dictionary.read();
            if let Some(trackable) = dict.get(key) {
                return match trackable.state {
                    TrackState::Deleted | TrackState::NotFound => None,
                    _ => Some(trackable.item.clone()),
                };
            }
        }

        // Try backing store
        if let Some(ref store_get) = self.store_get {
            if let Some(item) = store_get(key) {
                // Cache the result
                let mut dict = self.dictionary.write();
                dict.insert(key.clone(), Trackable::unchanged(item.clone()));
                return Some(item);
            }
        }

        // Mark as not found in cache
        {
            let mut dict = self.dictionary.write();
            dict.insert(
                key.clone(),
                Trackable::new(StorageItem::default(), TrackState::NotFound),
            );
        }

        None
    }

    /// Gets an item, returning an error if not found.
    pub fn get(&self, key: &StorageKey) -> DataCacheResult<StorageItem> {
        self.try_get(key).ok_or(DataCacheError::KeyNotFound)
    }

    /// Checks if a key exists in the cache or backing store.
    #[must_use]
    pub fn contains(&self, key: &StorageKey) -> bool {
        self.try_get(key).is_some()
    }

    /// Adds an item to the cache.
    ///
    /// If the key already exists, this will update it with `Changed` state.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        let mut dict = self.dictionary.write();
        let state = if dict.contains_key(&key) {
            TrackState::Changed
        } else {
            TrackState::Added
        };
        dict.insert(key.clone(), Trackable::new(value, state));

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key);
        }
    }

    /// Attempts to add an item, returning an error if read-only.
    pub fn try_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.add(key, value);
        Ok(())
    }

    /// Updates an existing item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        let mut dict = self.dictionary.write();
        let state = match dict.get(&key) {
            Some(existing) if existing.state == TrackState::Added => TrackState::Added,
            _ => TrackState::Changed,
        };
        dict.insert(key.clone(), Trackable::new(value, state));

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key);
        }
    }

    /// Attempts to update an item, returning an error if read-only.
    pub fn try_update(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.update(key, value);
        Ok(())
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        let mut dict = self.dictionary.write();
        if let Some(existing) = dict.get(key) {
            if existing.state == TrackState::Added {
                // Item was added and never committed, just remove it
                dict.remove(key);
                if let Some(ref change_set) = self.change_set {
                    change_set.write().remove(key);
                }
                return;
            }
        }

        dict.insert(key.clone(), Trackable::deleted());

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key.clone());
        }
    }

    /// Attempts to delete an item, returning an error if read-only.
    pub fn try_delete(&self, key: &StorageKey) -> DataCacheResult {
        if self.read_only {
            return Err(DataCacheError::ReadOnly);
        }
        self.delete(key);
        Ok(())
    }

    /// Returns all items that have been modified.
    #[must_use]
    pub fn tracked_items(&self) -> Vec<(StorageKey, Trackable)> {
        self.dictionary
            .read()
            .iter()
            .filter(|(_, t)| t.is_modified())
            .map(|(k, t)| (k.clone(), t.clone()))
            .collect()
    }

    /// Commits all changes, resetting tracking states.
    pub fn commit(&self) {
        let mut dict = self.dictionary.write();
        let keys_to_remove: Vec<StorageKey> = dict
            .iter()
            .filter_map(|(k, t)| {
                if t.state == TrackState::Deleted || t.state == TrackState::NotFound {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect();

        for key in keys_to_remove {
            dict.remove(&key);
        }

        // Reset all states to None
        for trackable in dict.values_mut() {
            trackable.state = TrackState::None;
        }

        if let Some(ref change_set) = self.change_set {
            change_set.write().clear();
        }
    }

    /// Returns the number of items in the cache (including deleted/not-found markers).
    #[must_use]
    pub fn len(&self) -> usize {
        self.dictionary.read().len()
    }

    /// Returns whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.dictionary.read().is_empty()
    }

    /// Clears all items from the cache.
    pub fn clear(&self) {
        self.dictionary.write().clear();
        if let Some(ref change_set) = self.change_set {
            change_set.write().clear();
        }
    }

    /// Returns the number of modified items.
    #[must_use]
    pub fn modified_count(&self) -> usize {
        self.dictionary
            .read()
            .values()
            .filter(|t| t.is_modified())
            .count()
    }

    /// Finds items matching a prefix in the cache and backing store.
    #[must_use]
    pub fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Vec<(StorageKey, StorageItem)> {
        let mut results: HashMap<StorageKey, StorageItem> = HashMap::new();

        // First get from backing store
        if let Some(ref store_find) = self.store_find {
            for (key, item) in store_find(prefix, direction) {
                results.insert(key, item);
            }
        }

        // Then overlay with cached values
        let dict = self.dictionary.read();
        for (key, trackable) in dict.iter() {
            if let Some(prefix_key) = prefix {
                if !key.key().starts_with(prefix_key.key()) {
                    continue;
                }
            }

            match trackable.state {
                TrackState::Deleted | TrackState::NotFound => {
                    results.remove(key);
                }
                _ => {
                    results.insert(key.clone(), trackable.item.clone());
                }
            }
        }

        let mut sorted: Vec<_> = results.into_iter().collect();
        match direction {
            SeekDirection::Forward => sorted.sort_by(|a, b| a.0.cmp(&b.0)),
            SeekDirection::Backward => sorted.sort_by(|a, b| b.0.cmp(&a.0)),
        }
        sorted
    }
}

impl std::fmt::Debug for DataCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let dict = self.dictionary.read();
        f.debug_struct("DataCache")
            .field("read_only", &self.read_only)
            .field("entries", &dict.len())
            .field(
                "modified",
                &dict.values().filter(|t| t.is_modified()).count(),
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ Constructor Tests ============

    #[test]
    fn test_new_writable() {
        let cache = DataCache::new(false);
        assert!(!cache.is_read_only());
        assert!(cache.is_empty());
    }

    #[test]
    fn test_new_read_only() {
        let cache = DataCache::new(true);
        assert!(cache.is_read_only());
    }

    #[test]
    fn test_new_with_store() {
        let store_get: Arc<StoreGetFn> = Arc::new(|_| Some(StorageItem::new(vec![0xAA])));
        let cache = DataCache::new_with_store(false, Some(store_get), None);
        assert!(!cache.is_read_only());
    }

    // ============ Basic Operations ============

    #[test]
    fn test_add_and_get() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA, 0xBB]);

        cache.add(key.clone(), value.clone());

        let result = cache.try_get(&key);
        assert!(result.is_some());
        assert_eq!(result.unwrap().value(), value.value());
    }

    #[test]
    fn test_try_get_nonexistent() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        assert!(cache.try_get(&key).is_none());
    }

    #[test]
    fn test_get_error_on_nonexistent() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        let result = cache.get(&key);
        assert!(matches!(result, Err(DataCacheError::KeyNotFound)));
    }

    #[test]
    fn test_contains() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        assert!(!cache.contains(&key));
        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        assert!(cache.contains(&key));
    }

    #[test]
    fn test_update() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        let value1 = StorageItem::new(vec![0xAA]);
        let value2 = StorageItem::new(vec![0xBB]);

        cache.add(key.clone(), value1);
        cache.update(key.clone(), value2.clone());

        assert_eq!(cache.try_get(&key).unwrap().value(), value2.value());
    }

    #[test]
    fn test_delete() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        let value = StorageItem::new(vec![0xAA]);

        cache.add(key.clone(), value);
        assert!(cache.contains(&key));

        cache.delete(&key);
        assert!(!cache.contains(&key));
    }

    // ============ Read-Only Tests ============

    #[test]
    fn test_try_add_read_only() {
        let cache = DataCache::new(true);
        let key = StorageKey::new(-1, vec![0x01]);
        let result = cache.try_add(key, StorageItem::new(vec![0xAA]));
        assert!(matches!(result, Err(DataCacheError::ReadOnly)));
    }

    #[test]
    fn test_try_update_read_only() {
        let cache = DataCache::new(true);
        let key = StorageKey::new(-1, vec![0x01]);
        let result = cache.try_update(key, StorageItem::new(vec![0xAA]));
        assert!(matches!(result, Err(DataCacheError::ReadOnly)));
    }

    #[test]
    fn test_try_delete_read_only() {
        let cache = DataCache::new(true);
        let key = StorageKey::new(-1, vec![0x01]);
        let result = cache.try_delete(&key);
        assert!(matches!(result, Err(DataCacheError::ReadOnly)));
    }

    // ============ Tracking Tests ============

    #[test]
    fn test_tracked_items() {
        let cache = DataCache::new(false);
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);

        cache.add(key1.clone(), StorageItem::new(vec![0xAA]));
        cache.add(key2.clone(), StorageItem::new(vec![0xBB]));

        let tracked = cache.tracked_items();
        assert_eq!(tracked.len(), 2);
    }

    #[test]
    fn test_modified_count() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        assert_eq!(cache.modified_count(), 0);
        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        assert_eq!(cache.modified_count(), 1);
    }

    #[test]
    fn test_commit_resets_state() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        assert_eq!(cache.modified_count(), 1);

        cache.commit();
        assert_eq!(cache.modified_count(), 0);
        assert!(cache.contains(&key)); // Still exists after commit
    }

    #[test]
    fn test_commit_removes_deleted() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        cache.commit(); // Now state is None
        cache.delete(&key);
        cache.commit(); // Should remove the entry

        assert_eq!(cache.len(), 0);
    }

    // ============ Delete Behavior Tests ============

    #[test]
    fn test_delete_added_item_removes_completely() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        cache.delete(&key);

        // Item should be completely removed (not marked deleted)
        // because it was never committed
        assert_eq!(cache.modified_count(), 0);
    }

    #[test]
    fn test_delete_committed_item_marks_deleted() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);

        cache.add(key.clone(), StorageItem::new(vec![0xAA]));
        cache.commit();
        cache.delete(&key);

        // Item should be marked deleted
        assert_eq!(cache.modified_count(), 1);
        assert!(!cache.contains(&key));
    }

    // ============ Store Delegation Tests ============

    #[test]
    fn test_try_get_from_store() {
        let expected_value = StorageItem::new(vec![0xCC, 0xDD]);
        let expected_value_clone = expected_value.clone();

        let store_get: Arc<StoreGetFn> = Arc::new(move |_key| Some(expected_value_clone.clone()));

        let cache = DataCache::new_with_store(false, Some(store_get), None);
        let key = StorageKey::new(-1, vec![0x01]);

        let result = cache.try_get(&key);
        assert!(result.is_some());
        assert_eq!(result.unwrap().value(), expected_value.value());
    }

    #[test]
    fn test_cache_overrides_store() {
        let store_value = StorageItem::new(vec![0xAA]);
        let cache_value = StorageItem::new(vec![0xBB]);
        let store_value_clone = store_value.clone();

        let store_get: Arc<StoreGetFn> = Arc::new(move |_key| Some(store_value_clone.clone()));

        let cache = DataCache::new_with_store(false, Some(store_get), None);
        let key = StorageKey::new(-1, vec![0x01]);

        // First get from store
        let _ = cache.try_get(&key);

        // Override with cache value
        cache.update(key.clone(), cache_value.clone());

        let result = cache.try_get(&key);
        assert_eq!(result.unwrap().value(), cache_value.value());
    }

    // ============ Utility Tests ============

    #[test]
    fn test_len_and_is_empty() {
        let cache = DataCache::new(false);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.add(
            StorageKey::new(-1, vec![0x01]),
            StorageItem::new(vec![0xAA]),
        );
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_clear() {
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::new(-1, vec![0x01]),
            StorageItem::new(vec![0xAA]),
        );
        cache.add(
            StorageKey::new(-1, vec![0x02]),
            StorageItem::new(vec![0xBB]),
        );

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_clone() {
        let cache = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        cache.add(key.clone(), StorageItem::new(vec![0xAA]));

        let cloned = cache.clone();
        assert!(cloned.contains(&key));

        // Modifying clone doesn't affect original
        cloned.delete(&key);
        assert!(cache.contains(&key));
        assert!(!cloned.contains(&key));
    }

    #[test]
    fn test_debug() {
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::new(-1, vec![0x01]),
            StorageItem::new(vec![0xAA]),
        );

        let debug = format!("{:?}", cache);
        assert!(debug.contains("DataCache"));
        assert!(debug.contains("entries"));
    }

    // ============ Find Tests ============

    #[test]
    fn test_find_in_cache() {
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::new(-1, vec![0x01, 0xAA]),
            StorageItem::new(vec![0x11]),
        );
        cache.add(
            StorageKey::new(-1, vec![0x01, 0xBB]),
            StorageItem::new(vec![0x22]),
        );
        cache.add(
            StorageKey::new(-1, vec![0x02, 0xAA]),
            StorageItem::new(vec![0x33]),
        );

        let results = cache.find(None, SeekDirection::Forward);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_find_excludes_deleted() {
        let cache = DataCache::new(false);
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);

        cache.add(key1.clone(), StorageItem::new(vec![0xAA]));
        cache.add(key2.clone(), StorageItem::new(vec![0xBB]));
        cache.commit();
        cache.delete(&key1);

        let results = cache.find(None, SeekDirection::Forward);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, key2);
    }

    // ============ Error Tests ============

    #[test]
    fn test_error_display() {
        assert_eq!(DataCacheError::ReadOnly.to_string(), "cache is read-only");
        assert_eq!(DataCacheError::KeyNotFound.to_string(), "key not found");
        assert!(
            DataCacheError::CommitFailed("test".to_string())
                .to_string()
                .contains("unable to commit")
        );
    }

    #[test]
    fn test_error_clone() {
        let err1 = DataCacheError::ReadOnly;
        let err2 = err1.clone();
        assert_eq!(err1, err2);
    }
}

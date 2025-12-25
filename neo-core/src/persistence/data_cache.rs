//! Shared in-memory cache for persistence providers.

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

pub struct DataCache {
    dictionary: Arc<RwLock<HashMap<StorageKey, Trackable>>>,
    change_set: Option<Arc<RwLock<HashSet<StorageKey>>>>,
    on_read: Arc<RwLock<Vec<OnEntryDelegate>>>,
    on_update: Arc<RwLock<Vec<OnEntryDelegate>>>,
    store_get: Option<Arc<StoreGetFn>>,
    store_find: Option<Arc<StoreFindFn>>,
}

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
            dictionary: Arc::clone(&self.dictionary),
            change_set: self.change_set.as_ref().map(Arc::clone),
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
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
        if self.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }
        self.add(key, value);
        Ok(())
    }

    /// Attempt to update an item in the cache, returning an error when read-only.
    pub fn try_update(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if self.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }
        self.update(key, value);
        Ok(())
    }

    /// Attempt to delete an item in the cache, returning an error when read-only.
    pub fn try_delete(&self, key: &StorageKey) -> DataCacheResult {
        if self.is_read_only() {
            return Err(DataCacheError::ReadOnly);
        }
        self.delete(key);
        Ok(())
    }

    /// Attempts to commit, returning an error when read-only.
    pub fn try_commit(&self) -> DataCacheResult {
        if self.is_read_only() {
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
            dictionary: Arc::new(RwLock::new(HashMap::new())),
            change_set: if read_only {
                None
            } else {
                Some(Arc::new(RwLock::new(HashSet::new())))
            },
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
        }
    }

    /// Returns true if DataCache is read-only.
    pub fn is_read_only(&self) -> bool {
        self.change_set.is_none()
    }

    /// Adds a handler for read events.
    pub fn on_read(&self, handler: OnEntryDelegate) {
        self.on_read.write().push(handler);
    }

    /// Adds a handler for update events.
    pub fn on_update(&self, handler: OnEntryDelegate) {
        self.on_update.write().push(handler);
    }

    /// Creates a deep copy of this cache, including tracked entries and change set state.
    pub fn clone_cache(&self) -> Self {
        let clone = DataCache::new_with_store(
            self.is_read_only(),
            self.store_get.as_ref().map(Arc::clone),
            self.store_find.as_ref().map(Arc::clone),
        );

        {
            clone.dictionary.write().extend(
                self.dictionary
                    .read()
                    .iter()
                    .map(|(key, trackable)| (key.clone(), trackable.clone())),
            );
        }

        if !self.is_read_only() {
            if let (Some(source), Some(target)) = (&self.change_set, &clone.change_set) {
                target.write().extend(source.read().iter().cloned());
            }
        }

        clone
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
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        let dict_guard = self.dictionary.read();
        if let Some(trackable) = dict_guard.get(key) {
            if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound {
                return Some(trackable.item.clone());
            }
            return None;
        }
        drop(dict_guard);

        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                {
                    let mut dict = self.dictionary.write();
                    dict.entry(key.clone())
                        .or_insert_with(|| Trackable::new(item.clone(), TrackState::None));
                }

                for handler in self.on_read.read().clone() {
                    handler(self, key, &item);
                }

                return Some(item);
            }
        }

        None
    }

    /// Adds an item to the cache.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        if self.is_read_only() {
            warn!("attempted to add to read-only DataCache");
            return;
        }
        self.apply_add(&key, value.clone());
        for handler in self.on_update.read().clone() {
            handler(self, &key, &value);
        }
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.is_read_only() {
            warn!("attempted to update read-only DataCache");
            return;
        }
        self.apply_update(&key, value.clone());

        // Trigger update event
        for handler in self.on_update.read().clone() {
            handler(self, &key, &value);
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.is_read_only() {
            warn!("attempted to delete from read-only DataCache");
            return;
        }
        self.apply_delete(key);
    }

    /// Commits changes to the underlying storage.
    /// Note: Calling commit on a read-only cache is a no-op (common in verification paths).
    pub fn commit(&self) {
        if self.is_read_only() {
            // Read-only caches are common during block verification; silently skip.
            tracing::trace!("commit called on read-only DataCache (expected in verification)");
            return;
        }

        // Persistence is handled by StoreCache/Snapshot; here we only clear tracked changes.
        if let Some(ref change_set) = self.change_set {
            change_set.write().clear();
        }
    }

    fn apply_add(&self, key: &StorageKey, value: StorageItem) -> bool {
        let trackable = Trackable::new(value, TrackState::Added);
        self.dictionary.write().insert(key.clone(), trackable);

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key.clone());
        }
        true
    }

    fn apply_update(&self, key: &StorageKey, value: StorageItem) {
        let mut dict = self.dictionary.write();
        if let Some(trackable) = dict.get_mut(key) {
            trackable.item = value.clone();
            if trackable.state == TrackState::None {
                trackable.state = TrackState::Changed;
            }
        } else {
            dict.insert(
                key.clone(),
                Trackable::new(value.clone(), TrackState::Changed),
            );
        }

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key.clone());
        }
    }

    fn apply_delete(&self, key: &StorageKey) {
        let mut dict = self.dictionary.write();
        if let Some(trackable) = dict.get_mut(key) {
            trackable.state = TrackState::Deleted;
        } else {
            dict.insert(
                key.clone(),
                Trackable::new(StorageItem::default(), TrackState::Deleted),
            );
        }

        if let Some(ref change_set) = self.change_set {
            change_set.write().insert(key.clone());
        }
    }

    /// Gets the change set.
    pub fn get_change_set(&self) -> Vec<StorageKey> {
        if let Some(ref change_set) = self.change_set {
            change_set.read().iter().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// Returns a snapshot of all tracked entries, typically used when
    /// propagating changes into an underlying store.
    pub fn tracked_items(&self) -> Vec<(StorageKey, Trackable)> {
        let dict = self.dictionary.read();
        if let Some(change_set) = &self.change_set {
            let keys: Vec<_> = change_set.read().iter().cloned().collect();
            keys.into_iter()
                .filter_map(|key| dict.get(&key).cloned().map(|track| (key, track)))
                .collect()
        } else {
            dict.iter()
                .map(|(key, track)| (key.clone(), track.clone()))
                .collect()
        }
    }
}

impl IReadOnlyStore for DataCache {}

/// Conversion utilities for extracting state changes from DataCache.
impl DataCache {
    /// Extracts all tracked changes as raw key-value pairs.
    ///
    /// Returns a vector of (key_bytes, value_bytes, is_deleted) tuples.
    /// This is useful for converting to other state change formats.
    ///
    /// # Returns
    /// - `key_bytes`: The full serialized storage key (id + suffix)
    /// - `value_bytes`: The storage item value (None if deleted)
    /// - Contract ID can be extracted from first 4 bytes of key_bytes
    pub fn extract_raw_changes(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let tracked = self.tracked_items();
        tracked
            .into_iter()
            .filter_map(|(key, trackable)| match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    Some((key.to_array(), Some(trackable.item.get_value())))
                }
                TrackState::Deleted => Some((key.to_array(), None)),
                TrackState::None | TrackState::NotFound => None,
            })
            .collect()
    }

    /// Returns the number of pending changes.
    pub fn pending_change_count(&self) -> usize {
        if let Some(ref change_set) = self.change_set {
            change_set.read().len()
        } else {
            0
        }
    }

    /// Returns true if there are any pending changes.
    pub fn has_pending_changes(&self) -> bool {
        self.pending_change_count() > 0
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
    fn extract_raw_changes_returns_added_and_changed() {
        let cache = DataCache::new(false);
        let key1 = make_key(1, b"add");
        let key2 = make_key(2, b"upd");

        cache.add(key1.clone(), StorageItem::from_bytes(vec![0xAA]));
        cache.add(key2.clone(), StorageItem::from_bytes(vec![0xBB]));
        cache.update(key2.clone(), StorageItem::from_bytes(vec![0xCC]));

        let changes = cache.extract_raw_changes();
        assert_eq!(changes.len(), 2);

        // Verify values are present
        let values: Vec<_> = changes.iter().filter_map(|(_, v)| v.clone()).collect();
        assert!(values.contains(&vec![0xAA]));
        assert!(values.contains(&vec![0xCC]));
    }

    #[test]
    fn extract_raw_changes_includes_deletions() {
        let cache = DataCache::new(false);
        let key = make_key(5, b"del");

        cache.add(key.clone(), StorageItem::from_bytes(vec![0x11]));
        cache.delete(&key);

        let changes = cache.extract_raw_changes();
        assert_eq!(changes.len(), 1);
        assert!(
            changes[0].1.is_none(),
            "deleted item should have None value"
        );
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
}

impl IReadOnlyStoreGeneric<StorageKey, StorageItem> for DataCache {
    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let mut combined: HashMap<StorageKey, StorageItem> = HashMap::new();

        let dict_guard = self.dictionary.read();
        for (key, trackable) in dict_guard.iter() {
            if trackable.state == TrackState::Deleted || trackable.state == TrackState::NotFound {
                continue;
            }

            if let Some(prefix) = key_prefix {
                if key.id != prefix.id || !key.suffix().starts_with(prefix.suffix()) {
                    continue;
                }
            }

            combined
                .entry(key.clone())
                .or_insert_with(|| trackable.item.clone());
        }
        drop(dict_guard);

        if let Some(finder) = &self.store_find {
            for (key, value) in finder(key_prefix, SeekDirection::Forward) {
                combined.entry(key).or_insert(value);
            }
        }

        let mut items: Vec<_> = combined.into_iter().collect();
        items.sort_by(|a, b| a.0.suffix().cmp(b.0.suffix()));

        if direction == SeekDirection::Backward {
            Box::new(items.into_iter().rev())
        } else {
            Box::new(items.into_iter())
        }
    }
}

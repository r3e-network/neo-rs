use super::storage_watch::log_watched_storage_event;
use super::trackable::{DataCacheConfig, DataCacheError, DataCacheResult, InnerState, Trackable};
use crate::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::types::{StorageItem, StorageKey, TrackState};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tracing::warn;

/// Delegate for storage entries
pub type OnEntryDelegate = Arc<dyn Fn(&DataCache, &StorageKey, &StorageItem) + Send + Sync>;

/// Represents a cache for the underlying storage of the NEO blockchain.
pub(crate) type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;
pub(crate) type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;
pub(crate) type CommitApplyFn = dyn Fn(&[(StorageKey, Trackable)]) + Send + Sync;

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
    /// Configuration
    config: DataCacheConfig,
    /// Optional commit sink used by cloned overlays to propagate tracked
    /// changes into their parent cache (mirrors Neo C# ClonedCache semantics).
    commit_apply: Option<Arc<CommitApplyFn>>,
}

fn key_matches_prefix(key: &StorageKey, prefix: Option<&[u8]>) -> bool {
    match prefix {
        Some(prefix) => key.as_bytes().starts_with(prefix),
        None => true,
    }
}

fn visible_trackable_item(trackable: &Trackable) -> Option<StorageItem> {
    match trackable.state {
        TrackState::Deleted | TrackState::NotFound => None,
        _ => Some(trackable.item.clone()),
    }
}

fn overlay_item(trackable: &Trackable) -> Option<Option<StorageItem>> {
    match trackable.state {
        TrackState::Added | TrackState::Changed => Some(Some(trackable.item.clone())),
        TrackState::Deleted | TrackState::NotFound => Some(None),
        TrackState::None => None,
    }
}

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
            config: self.config,
            commit_apply: self.commit_apply.as_ref().map(Arc::clone),
        }
    }
}

impl DataCache {
    /// Creates a new DataCache.
    pub fn new(read_only: bool) -> Self {
        Self::new_with_config(read_only, None, None, DataCacheConfig::default())
    }

    /// Creates a new DataCache with configuration.
    pub fn with_config(read_only: bool, config: DataCacheConfig) -> Self {
        Self::new_with_config(read_only, None, None, config)
    }

    /// Attempt to add an item to the cache, returning an error when read-only.
    pub fn try_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.ensure_writable()?;
        self.add_writable(key, value)
    }

    /// Attempt to update an item in the cache, returning an error when read-only.
    pub fn try_update(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.ensure_writable()?;
        self.update_writable(key, value);
        Ok(())
    }

    /// Attempt to delete an item in the cache, returning an error when read-only.
    pub fn try_delete(&self, key: &StorageKey) -> DataCacheResult {
        self.ensure_writable()?;
        self.delete_writable(key);
        Ok(())
    }

    /// Attempts to commit, returning an error when read-only.
    pub fn try_commit(&self) -> DataCacheResult {
        self.ensure_writable()?;
        self.commit_writable();
        Ok(())
    }

    fn ensure_writable(&self) -> DataCacheResult {
        if self.read_only {
            Err(DataCacheError::ReadOnly)
        } else {
            Ok(())
        }
    }

    /// Creates a new DataCache with an optional backing store.
    pub fn new_with_store(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
    ) -> Self {
        Self::new_with_config(read_only, store_get, store_find, DataCacheConfig::default())
    }

    /// Creates a new DataCache with configuration and optional backing store.
    pub fn new_with_config(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
        config: DataCacheConfig,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only,
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
            ref_count: Arc::new(AtomicUsize::new(1)),
            config,
            commit_apply: None,
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

    /// Creates a cloned overlay cache that uses this cache as the backing store.
    pub fn clone_cache(&self) -> Self {
        let parent = Arc::new(self.clone());
        let store_get_parent = Arc::clone(&parent);
        let store_find_parent = Arc::clone(&parent);
        let commit_parent = Arc::clone(&parent);

        let store_get: Arc<StoreGetFn> =
            Arc::new(move |key: &StorageKey| store_get_parent.get(key));
        let store_find: Arc<StoreFindFn> =
            Arc::new(move |prefix, direction| store_find_parent.find(prefix, direction).collect());

        let mut overlay =
            Self::new_with_config(false, Some(store_get), Some(store_find), self.config);
        overlay.commit_apply = Some(Arc::new(move |items: &[(StorageKey, Trackable)]| {
            commit_parent.merge_tracked_items(items);
        }));
        overlay
    }

    /// Merges tracked changes from another cache into this one.
    pub fn merge_tracked_items(&self, items: &[(StorageKey, Trackable)]) {
        for (key, trackable) in items {
            log_watched_storage_event(
                "merge",
                "merge_tracked_items",
                key,
                None,
                Some(trackable.state),
                Some(&trackable.item),
            );
            match trackable.state {
                TrackState::Added => self.add(key.clone(), trackable.item.clone()),
                TrackState::Changed => self.update(key.clone(), trackable.item.clone()),
                TrackState::Deleted => self.delete(key),
                TrackState::None | TrackState::NotFound => {}
            }
        }
    }

    /// Creates an isolated writable fork backed by the same underlying store.
    pub fn fork_isolated(&self) -> Self {
        let state = self.state.read();
        let cloned_state = InnerState {
            dictionary: state.dictionary.clone(),
            change_set: state.change_set.clone(),
        };
        drop(state);

        Self {
            state: Arc::new(RwLock::new(cloned_state)),
            read_only: self.read_only,
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
            ref_count: Arc::new(AtomicUsize::new(1)),
            config: self.config,
            commit_apply: None,
        }
    }

    /// Gets an item from the cache.
    #[inline]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        // First check write cache
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
                {
                    log_watched_storage_event(
                        "get",
                        "dictionary_hit",
                        key,
                        Some(trackable.state),
                        Some(trackable.state),
                        Some(&trackable.item),
                    );
                    return Some(trackable.item.clone());
                }
                log_watched_storage_event(
                    "get",
                    "dictionary_deleted",
                    key,
                    Some(trackable.state),
                    Some(trackable.state),
                    None,
                );
                return None;
            }
        }

        // Fall back to store getter
        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                self.track_in_write_cache(key, &item);

                for handler in self.on_read.read().iter() {
                    handler(self, key, &item);
                }

                log_watched_storage_event("get", "store_get_hit", key, None, None, Some(&item));
                return Some(item);
            }
        }

        log_watched_storage_event("get", "miss", key, None, None, None);
        None
    }

    fn track_in_write_cache(&self, key: &StorageKey, item: &StorageItem) {
        if !self.config.track_reads_in_write_cache {
            return;
        }
        let mut state = self.state.write();
        if state.dictionary.len() < self.config.max_entries {
            state
                .dictionary
                .entry(key.clone())
                .or_insert_with(|| Trackable::new(item.clone(), TrackState::None));
        }
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
        if self.try_add(key, value).is_err() {
            warn!("attempted to add to read-only DataCache");
        }
    }

    fn add_writable(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        self.apply_add(&key, value.clone())?;
        for handler in self.on_update.read().iter() {
            handler(self, &key, &value);
        }
        Ok(())
    }

    fn apply_add(&self, key: &StorageKey, value: StorageItem) -> DataCacheResult {
        let mut state = self.state.write();
        let prev_state = state
            .dictionary
            .get(key)
            .map(|t| t.state)
            .unwrap_or(TrackState::NotFound);
        let new_state = match prev_state {
            TrackState::Deleted => TrackState::Changed,
            TrackState::NotFound => TrackState::Added,
            TrackState::Added | TrackState::Changed | TrackState::None => {
                return Err(DataCacheError::InvalidState(prev_state));
            }
        };
        log_watched_storage_event(
            "add",
            "apply_add",
            key,
            Some(prev_state),
            Some(new_state),
            Some(&value),
        );
        state
            .dictionary
            .insert(key.clone(), Trackable::new(value, new_state));
        state.change_set.insert(key.clone());
        Ok(())
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.try_update(key, value).is_err() {
            warn!("attempted to update read-only DataCache");
        }
    }

    fn update_writable(&self, key: StorageKey, value: StorageItem) {
        self.apply_update(&key, value.clone());
        for handler in self.on_update.read().iter() {
            handler(self, &key, &value);
        }
    }

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
        log_watched_storage_event(
            "update",
            "apply_update",
            key,
            Some(prev_state),
            Some(new_state),
            Some(&value),
        );
        state
            .dictionary
            .insert(key.clone(), Trackable::new(value, new_state));
        state.change_set.insert(key.clone());
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.try_delete(key).is_err() {
            warn!("attempted to delete from read-only DataCache");
        }
    }

    fn delete_writable(&self, key: &StorageKey) {
        self.apply_delete(key);
    }

    fn apply_delete(&self, key: &StorageKey) {
        let (prev_state, previous_item) = {
            let state = self.state.read();
            let previous = state.dictionary.get(key);
            (
                previous.map(|t| t.state).unwrap_or(TrackState::NotFound),
                previous.map(|t| t.item.clone()),
            )
        };

        match prev_state {
            TrackState::Added => {
                let mut state = self.state.write();
                state.dictionary.remove(key);
                state.change_set.remove(key);
                log_watched_storage_event(
                    "delete",
                    "apply_delete_added",
                    key,
                    Some(prev_state),
                    Some(TrackState::NotFound),
                    previous_item.as_ref(),
                );
            }
            TrackState::Changed | TrackState::None => {
                let mut state = self.state.write();
                state.dictionary.insert(
                    key.clone(),
                    Trackable::new(StorageItem::default(), TrackState::Deleted),
                );
                state.change_set.insert(key.clone());
                log_watched_storage_event(
                    "delete",
                    "apply_delete_tracked",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    previous_item.as_ref(),
                );
            }
            TrackState::Deleted => {
                log_watched_storage_event(
                    "delete",
                    "apply_delete_already_deleted",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    previous_item.as_ref(),
                );
            }
            TrackState::NotFound => {
                let store_item = self.store_get.as_ref().and_then(|getter| getter(key));
                if store_item.is_none() {
                    log_watched_storage_event(
                        "delete",
                        "apply_delete_not_found_skip",
                        key,
                        Some(prev_state),
                        None,
                        None,
                    );
                    return;
                }

                let mut state = self.state.write();
                let current_state = state
                    .dictionary
                    .get(key)
                    .map(|t| t.state)
                    .unwrap_or(TrackState::NotFound);
                match current_state {
                    TrackState::Added => {
                        state.dictionary.remove(key);
                        state.change_set.remove(key);
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_added",
                            key,
                            Some(prev_state),
                            Some(TrackState::NotFound),
                            store_item.as_ref(),
                        );
                    }
                    TrackState::Changed | TrackState::None | TrackState::NotFound => {
                        state.dictionary.insert(
                            key.clone(),
                            Trackable::new(StorageItem::default(), TrackState::Deleted),
                        );
                        state.change_set.insert(key.clone());
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_emit",
                            key,
                            Some(prev_state),
                            Some(TrackState::Deleted),
                            store_item.as_ref(),
                        );
                    }
                    TrackState::Deleted => {
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_already_deleted",
                            key,
                            Some(prev_state),
                            Some(TrackState::Deleted),
                            store_item.as_ref(),
                        );
                    }
                }
            }
        }
    }

    /// Commits changes to the underlying store.
    pub fn commit(&self) {
        if self.read_only {
            return;
        }
        self.commit_writable();
    }

    fn commit_writable(&self) {
        if let Some(apply) = &self.commit_apply {
            let tracked = self.tracked_items();
            if !tracked.is_empty() {
                apply(&tracked);
            }
        }

        let mut state = self.state.write();
        let keys: Vec<StorageKey> = state.change_set.iter().cloned().collect();
        for key in keys {
            if let Some(trackable) = state.dictionary.get_mut(&key) {
                match trackable.state {
                    TrackState::Added | TrackState::Changed => {
                        trackable.state = TrackState::None;
                    }
                    TrackState::Deleted => {
                        state.dictionary.remove(&key);
                    }
                    TrackState::None | TrackState::NotFound => {}
                }
            }
        }
        state.change_set.clear();
    }

    /// Resets the cache for reuse.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.dictionary.clear();
        state.change_set.clear();
        drop(state);
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

    /// Gets the configuration.
    pub fn config(&self) -> &DataCacheConfig {
        &self.config
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.as_bytes().into_owned());

        if let Some(store_find) = &self.store_find {
            let state = self.state.read();
            let mut overlays = BTreeMap::new();
            for key in &state.change_set {
                if !key_matches_prefix(key, prefix_bytes.as_deref()) {
                    continue;
                }

                let Some(trackable) = state.dictionary.get(key) else {
                    continue;
                };
                if let Some(item) = overlay_item(trackable) {
                    overlays.insert(key.clone(), item);
                }
            }
            drop(state);

            if overlays.is_empty() {
                let prefix_bytes = prefix_bytes.clone();
                return Box::new(
                    store_find(key_prefix, direction)
                        .into_iter()
                        .filter(move |(key, _)| key_matches_prefix(key, prefix_bytes.as_deref())),
                );
            }

            let mut merged = BTreeMap::new();
            for (key, item) in store_find(key_prefix, direction) {
                if key_matches_prefix(&key, prefix_bytes.as_deref()) {
                    merged.insert(key, item);
                }
            }

            for (key, item) in overlays {
                match item {
                    Some(item) => {
                        merged.insert(key, item);
                    }
                    None => {
                        merged.remove(&key);
                    }
                }
            }

            return match direction {
                SeekDirection::Forward => Box::new(merged.into_iter()),
                SeekDirection::Backward => Box::new(merged.into_iter().rev()),
            };
        }

        let state = self.state.read();
        let base_items: BTreeMap<StorageKey, StorageItem> = state
            .dictionary
            .iter()
            .filter(|(key, _)| key_matches_prefix(key, prefix_bytes.as_deref()))
            .filter_map(|(key, trackable)| {
                visible_trackable_item(trackable).map(|item| (key.clone(), item))
            })
            .collect();

        match direction {
            SeekDirection::Forward => Box::new(base_items.into_iter()),
            SeekDirection::Backward => Box::new(base_items.into_iter().rev()),
        }
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
                            Some((key.to_array(), Some(trackable.item.to_value())))
                        }
                        TrackState::Deleted => Some((key.to_array(), None)),
                        TrackState::None | TrackState::NotFound => None,
                    })
            })
            .collect()
    }
}

impl ReadOnlyStore for DataCache {}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for DataCache {
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

#[cfg(test)]
mod tests;

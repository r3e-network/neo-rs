use super::prefetch::AccessPatternTracker;
use super::prefetch::PrefetchPattern;
use super::storage_watch::log_watched_storage_event;
use super::trackable::{DataCacheConfig, DataCacheError, DataCacheResult, InnerState, Trackable};
use crate::persistence::read_cache::{ReadCache, ReadCacheStatsSnapshot};
use crate::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::smart_contract::{StorageItem, StorageKey};
use parking_lot::RwLock;
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, trace, warn};

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
    /// Optional LRU read cache for frequently accessed keys
    read_cache: Option<Arc<ReadCache<StorageKey, StorageItem>>>,
    /// Configuration
    config: DataCacheConfig,
    /// Access pattern tracker for intelligent prefetching
    pattern_tracker: RwLock<AccessPatternTracker>,
    /// Global access sequence counter
    access_seq: AtomicU64,
    /// Prefetch window - keys that were recently prefetched
    prefetch_window: RwLock<HashSet<StorageKey>>,
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
        crate::persistence::track_state::TrackState::Deleted
        | crate::persistence::track_state::TrackState::NotFound => None,
        _ => Some(trackable.item.clone()),
    }
}

fn overlay_item(trackable: &Trackable) -> Option<Option<StorageItem>> {
    match trackable.state {
        crate::persistence::track_state::TrackState::Added
        | crate::persistence::track_state::TrackState::Changed => {
            Some(Some(trackable.item.clone()))
        }
        crate::persistence::track_state::TrackState::Deleted
        | crate::persistence::track_state::TrackState::NotFound => Some(None),
        crate::persistence::track_state::TrackState::None => None,
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
            read_cache: self.read_cache.clone(),
            config: self.config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(HashSet::new()),
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
        self.add_writable(key, value);
        Ok(())
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
        let read_cache = if config.enable_read_cache {
            Some(Arc::new(ReadCache::new(config.read_cache_config)))
        } else {
            None
        };

        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only,
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
            ref_count: Arc::new(AtomicUsize::new(1)),
            read_cache,
            config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(HashSet::new()),
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
    ///
    /// This matches Neo C# `DataCache.CloneCache()` semantics:
    /// - reads fall through to the parent cache,
    /// - writes are tracked locally,
    /// - `commit()` applies tracked changes into the parent cache.
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
                crate::persistence::track_state::TrackState::Added => {
                    self.add(key.clone(), trackable.item.clone())
                }
                crate::persistence::track_state::TrackState::Changed => {
                    self.update(key.clone(), trackable.item.clone())
                }
                crate::persistence::track_state::TrackState::Deleted => self.delete(key),
                crate::persistence::track_state::TrackState::None
                | crate::persistence::track_state::TrackState::NotFound => {}
            }
        }
    }

    /// Creates an isolated writable fork backed by the same underlying store
    /// callbacks, with an independent in-memory tracked state.
    ///
    /// This is intended for per-transaction execution where writes must not
    /// leak back unless explicitly merged.
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
            read_cache: self.read_cache.clone(),
            config: self.config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(HashSet::new()),
            commit_apply: None,
        }
    }

    /// Gets an item from the cache.
    #[inline]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        // Access-pattern tracking is only useful when prefetching via read cache.
        let should_track_pattern = self.config.enable_prefetching && self.read_cache.is_some();
        let pattern = if should_track_pattern {
            self.record_access_pattern(key)
        } else {
            PrefetchPattern::None
        };

        // First check write cache (for uncommitted changes) - minimal lock hold
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != crate::persistence::track_state::TrackState::Deleted
                    && trackable.state != crate::persistence::track_state::TrackState::NotFound
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

        // Check read cache for frequently accessed keys (bloom filter checked inside)
        if let Some(ref cache) = self.read_cache {
            if let Some(item) = cache.get(key) {
                // Check if this was a prefetch hit
                if self.is_recently_prefetched(key) {
                    cache.record_prefetch_hit();
                }

                // Trigger prefetching for sequential patterns
                if pattern != PrefetchPattern::None {
                    self.trigger_prefetch_if_needed(key, pattern);
                }

                log_watched_storage_event("get", "read_cache_hit", key, None, None, Some(&item));
                return Some(item);
            }
        }

        // Fall back to store getter
        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                // Cache in read cache for future access
                if let Some(ref cache) = self.read_cache {
                    let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    cache.put(key.clone(), item.clone(), size);
                }

                // Also track in write cache for consistency (but don't block on it)
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

    /// Track an item in the write cache without blocking the caller.
    fn track_in_write_cache(&self, key: &StorageKey, item: &StorageItem) {
        if !self.config.track_reads_in_write_cache {
            return;
        }
        let mut state = self.state.write();
        // Check if we're approaching the max entries limit
        if state.dictionary.len() < self.config.max_entries {
            state.dictionary.entry(key.clone()).or_insert_with(|| {
                Trackable::new(
                    item.clone(),
                    crate::persistence::track_state::TrackState::None,
                )
            });
        }
    }

    /// Gets an item from the cache as a reference.
    #[inline]
    pub fn get_ref(&self, key: &StorageKey) -> Option<StorageItem> {
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != crate::persistence::track_state::TrackState::Deleted
                    && trackable.state != crate::persistence::track_state::TrackState::NotFound
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

    fn add_writable(&self, key: StorageKey, value: StorageItem) {
        // Invalidate read cache for this key
        if let Some(ref cache) = self.read_cache {
            cache.remove(&key);
        }

        self.apply_add(&key, value.clone());
        for handler in self.on_update.read().iter() {
            handler(self, &key, &value);
        }
    }

    /// Applies an add operation to the internal storage.
    fn apply_add(&self, key: &StorageKey, value: StorageItem) {
        let mut state = self.state.write();
        let prev_state = state
            .dictionary
            .get(key)
            .map(|t| t.state)
            .unwrap_or(crate::persistence::track_state::TrackState::NotFound);
        log_watched_storage_event(
            "add",
            "apply_add",
            key,
            Some(prev_state),
            Some(crate::persistence::track_state::TrackState::Added),
            Some(&value),
        );
        state.dictionary.insert(
            key.clone(),
            Trackable::new(value, crate::persistence::track_state::TrackState::Added),
        );
        state.change_set.insert(key.clone());
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.try_update(key, value).is_err() {
            warn!("attempted to update read-only DataCache");
        }
    }

    fn update_writable(&self, key: StorageKey, value: StorageItem) {
        // Update read cache with new value
        if let Some(ref cache) = self.read_cache {
            let size = value.value_bytes().len() + std::mem::size_of::<StorageKey>();
            cache.put(key.clone(), value.clone(), size);
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
            .unwrap_or(crate::persistence::track_state::TrackState::NotFound);
        let new_state = match prev_state {
            crate::persistence::track_state::TrackState::Added => {
                crate::persistence::track_state::TrackState::Added
            }
            crate::persistence::track_state::TrackState::Changed
            | crate::persistence::track_state::TrackState::None => {
                crate::persistence::track_state::TrackState::Changed
            }
            crate::persistence::track_state::TrackState::Deleted => {
                crate::persistence::track_state::TrackState::Added
            }
            crate::persistence::track_state::TrackState::NotFound => {
                crate::persistence::track_state::TrackState::Changed
            }
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
        // Invalidate read cache for this key
        if let Some(ref cache) = self.read_cache {
            cache.remove(key);
        }

        self.apply_delete(key);
    }

    /// Applies a delete operation.
    fn apply_delete(&self, key: &StorageKey) {
        let (prev_state, previous_item) = {
            let state = self.state.read();
            let previous = state.dictionary.get(key);
            (
                previous
                    .map(|t| t.state)
                    .unwrap_or(crate::persistence::track_state::TrackState::NotFound),
                previous.map(|t| t.item.clone()),
            )
        };

        match prev_state {
            crate::persistence::track_state::TrackState::Added => {
                let mut state = self.state.write();
                state.dictionary.remove(key);
                state.change_set.remove(key);
                log_watched_storage_event(
                    "delete",
                    "apply_delete_added",
                    key,
                    Some(prev_state),
                    Some(crate::persistence::track_state::TrackState::NotFound),
                    previous_item.as_ref(),
                );
            }
            crate::persistence::track_state::TrackState::Changed
            | crate::persistence::track_state::TrackState::None => {
                let mut state = self.state.write();
                state.dictionary.insert(
                    key.clone(),
                    Trackable::new(
                        StorageItem::default(),
                        crate::persistence::track_state::TrackState::Deleted,
                    ),
                );
                state.change_set.insert(key.clone());
                log_watched_storage_event(
                    "delete",
                    "apply_delete_tracked",
                    key,
                    Some(prev_state),
                    Some(crate::persistence::track_state::TrackState::Deleted),
                    previous_item.as_ref(),
                );
            }
            crate::persistence::track_state::TrackState::Deleted => {
                log_watched_storage_event(
                    "delete",
                    "apply_delete_already_deleted",
                    key,
                    Some(prev_state),
                    Some(crate::persistence::track_state::TrackState::Deleted),
                    previous_item.as_ref(),
                );
            }
            crate::persistence::track_state::TrackState::NotFound => {
                // For overlays that do not track reads, only emit a delete when the
                // key actually exists in the backing store.
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
                    .unwrap_or(crate::persistence::track_state::TrackState::NotFound);
                match current_state {
                    crate::persistence::track_state::TrackState::Added => {
                        state.dictionary.remove(key);
                        state.change_set.remove(key);
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_added",
                            key,
                            Some(prev_state),
                            Some(crate::persistence::track_state::TrackState::NotFound),
                            store_item.as_ref(),
                        );
                    }
                    crate::persistence::track_state::TrackState::Changed
                    | crate::persistence::track_state::TrackState::None
                    | crate::persistence::track_state::TrackState::NotFound => {
                        state.dictionary.insert(
                            key.clone(),
                            Trackable::new(
                                StorageItem::default(),
                                crate::persistence::track_state::TrackState::Deleted,
                            ),
                        );
                        state.change_set.insert(key.clone());
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_emit",
                            key,
                            Some(prev_state),
                            Some(crate::persistence::track_state::TrackState::Deleted),
                            store_item.as_ref(),
                        );
                    }
                    crate::persistence::track_state::TrackState::Deleted => {
                        log_watched_storage_event(
                            "delete",
                            "apply_delete_not_found_already_deleted",
                            key,
                            Some(prev_state),
                            Some(crate::persistence::track_state::TrackState::Deleted),
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

        // Match C# DataCache.Commit() state transitions:
        // - Added/Changed become None (still cached),
        // - Deleted entries are removed from dictionary.
        let mut state = self.state.write();
        let keys: Vec<StorageKey> = state.change_set.iter().cloned().collect();
        for key in keys {
            if let Some(trackable) = state.dictionary.get_mut(&key) {
                match trackable.state {
                    crate::persistence::track_state::TrackState::Added
                    | crate::persistence::track_state::TrackState::Changed => {
                        trackable.state = crate::persistence::track_state::TrackState::None;
                    }
                    crate::persistence::track_state::TrackState::Deleted => {
                        state.dictionary.remove(&key);
                    }
                    crate::persistence::track_state::TrackState::None
                    | crate::persistence::track_state::TrackState::NotFound => {}
                }
            }
        }
        state.change_set.clear();
    }

    /// Resets the cache for reuse, clearing all tracked entries while retaining
    /// allocated capacity. Much cheaper than drop + new for repeated use within
    /// a block's transaction loop.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.dictionary.clear();
        state.change_set.clear();
        drop(state);
        *self.pattern_tracker.write() = AccessPatternTracker::new();
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

    /// Returns true if read caching is enabled.
    pub fn has_read_cache(&self) -> bool {
        self.read_cache.is_some()
    }

    /// Gets read cache statistics if caching is enabled.
    pub fn read_cache_stats(&self) -> Option<ReadCacheStatsSnapshot> {
        self.read_cache.as_ref().map(|c| c.stats())
    }

    /// Clears the read cache.
    pub fn clear_read_cache(&self) {
        if let Some(ref cache) = self.read_cache {
            cache.clear();
            debug!(target: "neo", "DataCache read cache cleared");
        }
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

            // Fast path: no pending overlays for this prefix, so return the
            // backing iterator with explicit prefix filtering. Some backing
            // iterators are range scans starting at `key_prefix` and do not
            // enforce prefix boundaries by themselves.
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
                        crate::persistence::track_state::TrackState::Added
                        | crate::persistence::track_state::TrackState::Changed => {
                            Some((key.to_array(), Some(trackable.item.to_value())))
                        }
                        crate::persistence::track_state::TrackState::Deleted => {
                            Some((key.to_array(), None))
                        }
                        crate::persistence::track_state::TrackState::None
                        | crate::persistence::track_state::TrackState::NotFound => None,
                    })
            })
            .collect()
    }

    /// Records an access pattern for intelligent prefetching.
    pub(super) fn record_access_pattern(&self, key: &StorageKey) -> PrefetchPattern {
        let seq = self.access_seq.fetch_add(1, Ordering::Relaxed);
        self.pattern_tracker.write().record_access(key, seq)
    }

    /// Gets the current detected prefetch pattern.
    pub fn current_prefetch_pattern(&self) -> PrefetchPattern {
        self.pattern_tracker.read().current_pattern(30)
    }

    /// Checks if a key is in the prefetch window (recently prefetched).
    pub fn is_recently_prefetched(&self, key: &StorageKey) -> bool {
        self.prefetch_window.read().contains(key)
    }

    /// Clears the prefetch window.
    pub fn clear_prefetch_window(&self) {
        self.prefetch_window.write().clear();
    }

    /// Trigger prefetching based on detected access pattern.
    pub(super) fn trigger_prefetch_if_needed(&self, key: &StorageKey, pattern: PrefetchPattern) {
        if !self.config.enable_prefetching {
            return;
        }

        match pattern {
            PrefetchPattern::SequentialForward => {
                self.prefetch_next_keys(key, self.config.prefetch_count);
            }
            PrefetchPattern::SequentialBackward => {
                self.prefetch_prev_keys(key, self.config.prefetch_count);
            }
            _ => {}
        }
    }

    /// Prefetch next sequential keys.
    fn prefetch_next_keys(&self, key: &StorageKey, count: usize) {
        if let Some(ref store_find) = self.store_find {
            let items: Vec<(StorageKey, StorageItem)> =
                store_find(Some(key), SeekDirection::Forward)
                    .into_iter()
                    .filter(|(k, _)| !self.is_recently_prefetched(k))
                    .take(count)
                    .collect();

            if !items.is_empty() {
                {
                    let mut window = self.prefetch_window.write();
                    for (k, _) in &items {
                        window.insert(k.clone());
                    }
                    if window.len() > 1000 {
                        window.clear();
                    }
                }

                self.prefetch(items);
                trace!(target: "neo", count, "prefetched forward sequential keys");
            }
        }
    }

    /// Prefetch previous sequential keys.
    fn prefetch_prev_keys(&self, key: &StorageKey, count: usize) {
        if let Some(ref store_find) = self.store_find {
            let items: Vec<(StorageKey, StorageItem)> =
                store_find(Some(key), SeekDirection::Backward)
                    .into_iter()
                    .filter(|(k, _)| !self.is_recently_prefetched(k))
                    .take(count)
                    .collect();

            if !items.is_empty() {
                {
                    let mut window = self.prefetch_window.write();
                    for (k, _) in &items {
                        window.insert(k.clone());
                    }
                    if window.len() > 1000 {
                        window.clear();
                    }
                }

                self.prefetch(items);
                trace!(target: "neo", count, "prefetched backward sequential keys");
            }
        }
    }

    /// Pre-fetches items into the read cache.
    pub fn prefetch(&self, items: Vec<(StorageKey, StorageItem)>) {
        if let Some(ref cache) = self.read_cache {
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = v.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    (k, v, size)
                })
                .collect();
            cache.put_batch(cache_items);
        }
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

//! Shared in-memory cache for persistence providers.
//!
//! This module implements a Copy-on-Write (CoW) DataCache pattern for optimal
//! performance during block synchronization with optional LRU read caching
//! and intelligent prefetching for common access patterns.

use super::{
    i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric},
    read_cache::{ReadCache, ReadCacheConfig, ReadCacheStatsSnapshot},
    seek_direction::SeekDirection,
    track_state::TrackState,
};
use crate::smart_contract::{StorageItem, StorageKey};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace, warn};

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

/// Configuration for DataCache.
#[derive(Debug, Clone, Copy)]
pub struct DataCacheConfig {
    /// Maximum number of entries in the write cache
    pub max_entries: usize,
    /// Whether reads should be mirrored into the write cache dictionary.
    pub track_reads_in_write_cache: bool,
    /// Enable read caching with LRU
    pub enable_read_cache: bool,
    /// Read cache configuration
    pub read_cache_config: ReadCacheConfig,
    /// Enable intelligent prefetching based on access patterns
    pub enable_prefetching: bool,
    /// Number of items to prefetch when pattern detected
    pub prefetch_count: usize,
    /// Minimum confidence threshold for prefetching (0-100)
    pub prefetch_confidence_threshold: u8,
}

impl Default for DataCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 100000,
            track_reads_in_write_cache: true,
            enable_read_cache: true,
            read_cache_config: ReadCacheConfig::default(),
            enable_prefetching: true,
            prefetch_count: 10,
            prefetch_confidence_threshold: 30,
        }
    }
}

impl InnerState {
    fn new() -> Self {
        Self {
            dictionary: HashMap::new(),
            change_set: HashSet::new(),
        }
    }
}

/// Prefetch pattern detection for sequential access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchPattern {
    /// No prefetching.
    None,
    /// Sequential forward access (ascending keys).
    SequentialForward,
    /// Sequential backward access (descending keys).
    SequentialBackward,
    /// Strided access (fixed offset between keys).
    Strided,
}

/// Tracks access patterns for intelligent prefetching.
struct AccessPatternTracker {
    /// Last accessed key (for pattern detection)
    last_key: Option<StorageKey>,
    /// Last access sequence number
    last_seq: u64,
    /// Detected pattern
    pattern: PrefetchPattern,
    /// Confidence score (0-100)
    confidence: u8,
    /// Sequential access counter
    sequential_count: u32,
}

impl AccessPatternTracker {
    fn new() -> Self {
        Self {
            last_key: None,
            last_seq: 0,
            pattern: PrefetchPattern::None,
            confidence: 0,
            sequential_count: 0,
        }
    }

    /// Record an access and update pattern detection.
    fn record_access(&mut self, key: &StorageKey, seq: u64) -> PrefetchPattern {
        if let Some(ref last) = self.last_key {
            let key_bytes = key.to_array();
            let last_bytes = last.to_array();

            // Check for sequential access patterns
            if key_bytes > last_bytes {
                // Potential forward sequential
                if self.pattern == PrefetchPattern::SequentialForward {
                    self.sequential_count += 1;
                    self.confidence = (self.confidence + 10).min(100);
                } else {
                    self.pattern = PrefetchPattern::SequentialForward;
                    self.sequential_count = 1;
                    self.confidence = 20;
                }
            } else if key_bytes < last_bytes {
                // Potential backward sequential
                if self.pattern == PrefetchPattern::SequentialBackward {
                    self.sequential_count += 1;
                    self.confidence = (self.confidence + 10).min(100);
                } else {
                    self.pattern = PrefetchPattern::SequentialBackward;
                    self.sequential_count = 1;
                    self.confidence = 20;
                }
            } else {
                // No pattern or reset
                self.confidence = self.confidence.saturating_sub(5);
                if self.confidence < 10 {
                    self.pattern = PrefetchPattern::None;
                    self.sequential_count = 0;
                }
            }
        }

        self.last_key = Some(key.clone());
        self.last_seq = seq;
        self.pattern
    }

    /// Get the current detected pattern if confidence is high enough.
    fn current_pattern(&self, threshold: u8) -> PrefetchPattern {
        if self.confidence >= threshold {
            self.pattern
        } else {
            PrefetchPattern::None
        }
    }

    #[allow(dead_code)]
    fn reset(&mut self) {
        *self = Self::new();
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
            read_cache: self.read_cache.clone(),
            config: self.config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(HashSet::new()),
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
        }
    }

    /// Records an access pattern for intelligent prefetching.
    fn record_access_pattern(&self, key: &StorageKey) -> PrefetchPattern {
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
            read_cache: self.read_cache.clone(),
            config: self.config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(HashSet::new()),
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
                if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
                {
                    return Some(trackable.item.clone());
                }
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

                return Some(item);
            }
        }

        // Fall back to store getter
        if let Some(getter) = &self.store_get {
            if let Some(item) = getter(key) {
                // Cache in read cache for future access
                if let Some(ref cache) = self.read_cache {
                    let size = item.get_value().len() + std::mem::size_of::<StorageKey>();
                    cache.put(key.clone(), item.clone(), size);
                }

                // Also track in write cache for consistency (but don't block on it)
                self.track_in_write_cache(key, &item);

                for handler in self.on_read.read().iter() {
                    handler(self, key, &item);
                }

                return Some(item);
            }
        }

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
            state
                .dictionary
                .entry(key.clone())
                .or_insert_with(|| Trackable::new(item.clone(), TrackState::None));
        }
    }

    /// Trigger prefetching based on detected access pattern.
    fn trigger_prefetch_if_needed(&self, key: &StorageKey, pattern: PrefetchPattern) {
        if !self.config.enable_prefetching {
            return;
        }

        match pattern {
            PrefetchPattern::SequentialForward => {
                // Prefetch next keys in sequence
                self.prefetch_next_keys(key, self.config.prefetch_count);
            }
            PrefetchPattern::SequentialBackward => {
                // Prefetch previous keys in sequence
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
                // Mark these as prefetched
                {
                    let mut window = self.prefetch_window.write();
                    for (k, _) in &items {
                        window.insert(k.clone());
                    }
                    // Limit window size
                    if window.len() > 1000 {
                        window.clear(); // Simple eviction: clear when too large
                    }
                }

                // Prefetch into read cache
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
                // Mark these as prefetched
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

        // Update read cache with new value
        if let Some(ref cache) = self.read_cache {
            let size = value.get_value().len() + std::mem::size_of::<StorageKey>();
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

        // Invalidate read cache for this key
        if let Some(ref cache) = self.read_cache {
            cache.remove(key);
        }

        self.apply_delete(key);
    }

    /// Applies a delete operation.
    fn apply_delete(&self, key: &StorageKey) {
        let prev_state = {
            let state = self.state.read();
            state
                .dictionary
                .get(key)
                .map(|t| t.state)
                .unwrap_or(TrackState::NotFound)
        };

        match prev_state {
            TrackState::Added => {
                let mut state = self.state.write();
                state.dictionary.remove(key);
                state.change_set.remove(key);
            }
            TrackState::Changed | TrackState::None => {
                let mut state = self.state.write();
                state.dictionary.insert(
                    key.clone(),
                    Trackable::new(StorageItem::default(), TrackState::Deleted),
                );
                state.change_set.insert(key.clone());
            }
            TrackState::Deleted => {}
            TrackState::NotFound => {
                // For overlays that do not track reads, only emit a delete when the
                // key actually exists in the backing store.
                let exists_in_store = self
                    .store_get
                    .as_ref()
                    .and_then(|getter| getter(key))
                    .is_some();
                if !exists_in_store {
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
                    }
                    TrackState::Changed | TrackState::None | TrackState::NotFound => {
                        state.dictionary.insert(
                            key.clone(),
                            Trackable::new(StorageItem::default(), TrackState::Deleted),
                        );
                        state.change_set.insert(key.clone());
                    }
                    TrackState::Deleted => {}
                }
            }
        }
    }

    /// Commits changes to the underlying store.
    pub fn commit(&self) {
        if self.read_only {
            return;
        }
        // Clear change set (actual persistence handled by StoreCache)
        self.state.write().change_set.clear();

        // Note: We don't clear the read cache on commit -
        // it contains valid data that may be useful for future reads
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

    /// Pre-fetches items into the read cache.
    pub fn prefetch(&self, items: Vec<(StorageKey, StorageItem)>) {
        if let Some(ref cache) = self.read_cache {
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = v.get_value().len() + std::mem::size_of::<StorageKey>();
                    (k, v, size)
                })
                .collect();
            cache.put_batch(cache_items);
        }
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        let prefix_bytes = key_prefix.map(|k| k.to_array());

        if let Some(store_find) = &self.store_find {
            // Overlay only pending changes from the change-set onto backing-store
            // results. This avoids scanning the full dictionary (which may contain
            // many read-tracked keys) and correctly applies deletes.
            let state = self.state.read();
            let mut overlays: HashMap<StorageKey, Option<StorageItem>> = HashMap::new();

            for key in &state.change_set {
                if let Some(prefix) = &prefix_bytes {
                    if !key.to_array().starts_with(prefix) {
                        continue;
                    }
                }

                let Some(trackable) = state.dictionary.get(key) else {
                    continue;
                };
                match trackable.state {
                    TrackState::Added | TrackState::Changed => {
                        overlays.insert(key.clone(), Some(trackable.item.clone()));
                    }
                    TrackState::Deleted | TrackState::NotFound => {
                        overlays.insert(key.clone(), None);
                    }
                    TrackState::None => {}
                }
            }
            drop(state);

            // Fast path: no pending overlays for this prefix, so return the
            // backing iterator materialized as-is.
            if overlays.is_empty() {
                return Box::new(store_find(key_prefix, direction).into_iter());
            }

            let mut merged = Vec::new();
            for (key, item) in store_find(key_prefix, direction) {
                match overlays.remove(&key) {
                    Some(Some(overlay_item)) => merged.push((key, overlay_item)),
                    Some(None) => {}
                    None => merged.push((key, item)),
                }
            }

            merged.extend(
                overlays
                    .into_iter()
                    .filter_map(|(key, item)| item.map(|value| (key, value))),
            );
            match direction {
                SeekDirection::Forward => merged.sort_by(|a, b| a.0.cmp(&b.0)),
                SeekDirection::Backward => merged.sort_by(|a, b| b.0.cmp(&a.0)),
            }
            return Box::new(merged.into_iter());
        }

        let state = self.state.read();
        let mut base_items: Vec<(StorageKey, StorageItem)> = state
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

        match direction {
            SeekDirection::Forward => base_items.sort_by(|a, b| a.0.cmp(&b.0)),
            SeekDirection::Backward => base_items.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        Box::new(base_items.into_iter())
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
    use std::sync::Arc;

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

    #[test]
    fn delete_uncached_key_without_store_is_noop() {
        let cache = DataCache::new(false);
        let key = make_key(5, b"missing");
        cache.delete(&key);
        assert!(cache.tracked_items().is_empty());
    }

    #[test]
    fn delete_marks_uncached_key_as_deleted_when_backing_store_has_key() {
        let key = make_key(5, b"exists");
        let mut backing_map = std::collections::HashMap::new();
        backing_map.insert(key.clone(), StorageItem::from_bytes(vec![1]));
        let backing_map = Arc::new(backing_map);

        let getter = {
            let map = Arc::clone(&backing_map);
            Arc::new(move |lookup: &StorageKey| map.get(lookup).cloned())
        };

        let cache = DataCache::new_with_store(false, Some(getter), None);
        cache.delete(&key);

        let tracked = cache.tracked_items();
        assert_eq!(tracked.len(), 1, "delete should produce one tracked change");
        assert_eq!(tracked[0].0, key);
        assert_eq!(tracked[0].1.state, TrackState::Deleted);
    }

    #[test]
    fn find_overlays_changes_and_hides_deleted_store_entries() {
        let key_a = make_key(11, b"a");
        let key_b = make_key(11, b"b");
        let key_c = make_key(11, b"c");

        let mut backing_map = std::collections::HashMap::new();
        backing_map.insert(key_a.clone(), StorageItem::from_bytes(vec![1]));
        backing_map.insert(key_b.clone(), StorageItem::from_bytes(vec![2]));
        let backing_map = Arc::new(backing_map);

        let getter = {
            let map = Arc::clone(&backing_map);
            Arc::new(move |key: &StorageKey| map.get(key).cloned())
        };
        let finder = {
            let map = Arc::clone(&backing_map);
            Arc::new(
                move |prefix: Option<&StorageKey>,
                      direction: SeekDirection|
                      -> Vec<(StorageKey, StorageItem)> {
                    let prefix_bytes = prefix.map(|p| p.to_array());
                    let mut items: Vec<_> = map
                        .iter()
                        .filter(|(key, _)| match &prefix_bytes {
                            Some(bytes) => key.to_array().starts_with(bytes),
                            None => true,
                        })
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect();

                    match direction {
                        SeekDirection::Forward => items.sort_by(|a, b| a.0.cmp(&b.0)),
                        SeekDirection::Backward => items.sort_by(|a, b| b.0.cmp(&a.0)),
                    }

                    items
                },
            )
        };

        let cache = DataCache::new_with_store(false, Some(getter), Some(finder));
        cache.update(key_a.clone(), StorageItem::from_bytes(vec![9]));
        cache.delete(&key_b);
        cache.add(key_c.clone(), StorageItem::from_bytes(vec![3]));

        let prefix = make_key(11, b"");
        let entries: Vec<_> = cache.find(Some(&prefix), SeekDirection::Forward).collect();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, key_a);
        assert_eq!(entries[0].1.get_value(), vec![9]);
        assert_eq!(entries[1].0, key_c);
        assert_eq!(entries[1].1.get_value(), vec![3]);
    }
}

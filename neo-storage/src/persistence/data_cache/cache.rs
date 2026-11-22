use super::prefetch::AccessPatternTracker;
use super::prefetch::{PrefetchConfig, PrefetchMetrics, PrefetchMetricsSnapshot};
use super::prefetch::{PrefetchPattern, PrefetchScanPlan, PrefetchWindow};
use super::storage_watch::log_watched_storage_event;
use super::trackable::{DataCacheConfig, DataCacheError, DataCacheResult, InnerState, Trackable};
use crate::persistence::hot_key_cache::{
    HOT_KEY_CACHE_SHARDS, HotKeyCache, HotKeyCacheStatsSnapshot,
};
use crate::persistence::read_cache::{ReadCache, ReadCacheStatsSnapshot};
use crate::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::types::{StorageItem, StorageKey, TrackState};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tracing::{debug, trace, warn};

mod commit;
mod construct;
mod prefetch;
#[cfg(test)]
mod tests;
mod traits;

/// Delegate for storage entries
pub type OnEntryDelegate =
    Arc<dyn Fn(&PersistenceDataCache, &StorageKey, &StorageItem) + Send + Sync>;

/// Represents a cache for the underlying storage of the NEO blockchain.
pub(crate) type StoreGetFn = dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync;
pub(crate) type StoreFindFn =
    dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync;
/// Bounded scan delegate: `(prefix, exclusive cursor, direction, limit)`.
///
/// Prefetch asks the backing store for at most `limit` rows inside `prefix`
/// instead of collecting a whole prefix range and discarding the tail.
pub(crate) type StoreSeekFn = dyn Fn(
        Option<&StorageKey>,
        Option<&StorageKey>,
        SeekDirection,
        usize,
    ) -> Vec<(StorageKey, StorageItem)>
    + Send
    + Sync;
pub(crate) type CommitApplyFn = dyn Fn(&[(StorageKey, Trackable)]) + Send + Sync;
/// Monotonic version probe of a mutable backing store. Values must change on
/// every commit that can alter previously read entries.
pub(crate) type GenerationSource = dyn Fn() -> u64 + Send + Sync;

/// Represents a cache for the underlying storage of the NEO blockchain.
pub struct PersistenceDataCache {
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
    /// Optional bounded store scan used by speculative prefetch
    store_seek: Option<Arc<StoreSeekFn>>,
    /// Strong count for CoW detection
    ref_count: Arc<AtomicUsize>,
    /// Optional LRU read cache for frequently accessed keys
    read_cache: Option<Arc<ReadCache<StorageKey, StorageItem>>>,
    /// Optional sharded hot-key cache for committed reads
    hot_key_cache: Option<Arc<HotKeyCache>>,
    /// Configuration
    config: DataCacheConfig,
    /// Access pattern tracker for intelligent prefetching
    pattern_tracker: RwLock<AccessPatternTracker>,
    /// Global access sequence counter
    access_seq: AtomicU64,
    /// Bounded window of keys that were prefetched and not yet consumed
    prefetch_window: RwLock<PrefetchWindow>,
    /// Counters describing what speculative prefetching actually did
    prefetch_metrics: Arc<PrefetchMetrics>,
    /// Optional commit sink used by cloned overlays to propagate tracked
    /// changes into their parent cache (mirrors Neo C# ClonedCache semantics).
    commit_apply: Option<Arc<CommitApplyFn>>,
    /// Optional backing-store version probe. Store-backed caches set this so
    /// reads served from mirrors/LRU cannot outlive an external commit.
    generation_source: Option<Arc<GenerationSource>>,
    /// Last backing-store generation this cache's read state was validated for.
    observed_generation: AtomicU64,
}

fn key_matches_prefix(key: &StorageKey, prefix: Option<&[u8]>) -> bool {
    // An absent prefix matches every key.
    prefix.is_none_or(|prefix| key.as_bytes().starts_with(prefix))
}

/// Capacity of the prefetch window: two scans' worth of keys, never unbounded.
fn prefetch_window_capacity(config: &DataCacheConfig) -> usize {
    config
        .prefetch
        .limits
        .max_admit_keys
        .saturating_mul(2)
        .clamp(8, 4096)
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

impl Clone for PersistenceDataCache {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            read_only: self.read_only,
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
            store_seek: self.store_seek.as_ref().map(Arc::clone),
            ref_count: Arc::clone(&self.ref_count),
            read_cache: self.read_cache.clone(),
            hot_key_cache: self.hot_key_cache.clone(),
            config: self.config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(PrefetchWindow::new(self.prefetch_window_capacity())),
            prefetch_metrics: Arc::clone(&self.prefetch_metrics),
            commit_apply: self.commit_apply.as_ref().map(Arc::clone),
            generation_source: self.generation_source.as_ref().map(Arc::clone),
            observed_generation: AtomicU64::new(self.observed_generation.load(Ordering::Acquire)),
        }
    }
}

impl PersistenceDataCache {
    /// Creates a new PersistenceDataCache.
    pub fn new(read_only: bool) -> Self {
        Self::new_with_config(read_only, None, None, DataCacheConfig::default())
    }

    /// Creates a new PersistenceDataCache with configuration.
    pub fn with_config(read_only: bool, config: DataCacheConfig) -> Self {
        Self::new_with_config(read_only, None, None, config)
    }

    /// Gets an item from the cache.
    #[inline]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.refresh_if_store_changed();

        // A disabled prefetch configuration costs one predictable branch here
        // and never touches the tracker, the window or the counters.
        let prefetch_plan = if self.prefetch_active() {
            self.record_access_pattern(key)
        } else {
            None
        };

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

        // Check the sharded hot-key cache. It only ever retains values read
        // from committed backing state, so a hit needs neither the global
        // read-cache lock nor a storage trip.
        if let Some(ref hot) = self.hot_key_cache
            && let Some(item) = hot.get(key)
        {
            if self.take_prefetched(key) {
                self.prefetch_metrics.record_useful_hit();
            }

            // No read mirror is added here: for a hot key the hot cache *is*
            // the read mirror, and mirroring it would make every first read
            // take the dictionary's exclusive lock for a value this hit already
            // serves.
            for handler in self.on_read.read().iter() {
                handler(self, key, &item);
            }

            if let Some(plan) = prefetch_plan {
                self.run_prefetch(plan);
            }

            log_watched_storage_event("get", "hot_key_cache_hit", key, None, None, Some(&item));
            return Some(item);
        }

        // Check read cache
        if let Some(ref cache) = self.read_cache
            && let Some(item) = cache.get(key)
        {
            if self.take_prefetched(key) {
                cache.record_prefetch_hit();
                self.prefetch_metrics.record_useful_hit();
            }

            if let Some(plan) = prefetch_plan {
                self.run_prefetch(plan);
            }

            log_watched_storage_event("get", "read_cache_hit", key, None, None, Some(&item));
            return Some(item);
        }

        // Fall back to store getter
        if let Some(getter) = &self.store_get
            && let Some(item) = getter(key)
        {
            if let Some(ref cache) = self.read_cache {
                let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
                cache.put(key.clone(), item.clone(), size);
            }

            // A value just read from committed state is what the hot cache is
            // for. When it is retained there, the write dictionary keeps only
            // pending writes instead of mirroring every cold read.
            let retained_in_hot_cache = if let Some(ref hot) = self.hot_key_cache {
                hot.insert(key.clone(), item.clone());
                true
            } else {
                false
            };
            if !retained_in_hot_cache {
                self.track_in_write_cache(key, &item);
            }

            for handler in self.on_read.read().iter() {
                handler(self, key, &item);
            }

            log_watched_storage_event("get", "store_get_hit", key, None, None, Some(&item));

            // A run of cold reads is exactly the pattern speculative prefetch
            // exists for, so the scan is evaluated here too instead of only on
            // cache hits.
            if let Some(plan) = prefetch_plan {
                self.run_prefetch(plan);
            }

            return Some(item);
        }

        log_watched_storage_event("get", "miss", key, None, None, None);

        // A miss still advances the access pattern: skipping it here would make
        // the feature depend on some earlier read having warmed the cache.
        if let Some(plan) = prefetch_plan {
            self.run_prefetch(plan);
        }

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
        self.refresh_if_store_changed();
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key)
                && trackable.state != TrackState::Deleted
                && trackable.state != TrackState::NotFound
            {
                return Some(trackable.item.clone());
            }
        }
        None
    }

    /// Gets all tracked items for persistence.
    pub fn tracked_items(&self) -> Vec<(StorageKey, Trackable)> {
        let state = self.state.read();
        state
            .dictionary
            .iter()
            .filter(|(_, trackable)| {
                matches!(
                    trackable.state,
                    TrackState::Added | TrackState::Changed | TrackState::Deleted
                )
            })
            .map(|(key, trackable)| (key.clone(), trackable.clone()))
            .collect()
    }

    /// Iterates over all modified entries (`Added`, `Changed`, `Deleted`) without cloning.
    pub fn for_each_tracked_change<F>(&self, mut f: F)
    where
        F: FnMut(&StorageKey, &StorageItem, TrackState),
    {
        let state = self.state.read();
        for (key, trackable) in &state.dictionary {
            if matches!(
                trackable.state,
                TrackState::Added | TrackState::Changed | TrackState::Deleted
            ) {
                f(key, &trackable.item, trackable.state);
            }
        }
    }

    /// Gets the change set.
    pub fn get_change_set(&self) -> Vec<StorageKey> {
        self.state
            .read()
            .dictionary
            .iter()
            .filter(|(_, trackable)| {
                matches!(
                    trackable.state,
                    TrackState::Added | TrackState::Changed | TrackState::Deleted
                )
            })
            .map(|(key, _)| key.clone())
            .collect()
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

    /// Returns true if the sharded hot-key cache is enabled.
    pub fn has_hot_key_cache(&self) -> bool {
        self.hot_key_cache.is_some()
    }

    /// Gets hot-key cache statistics if that cache is enabled.
    pub fn hot_key_cache_stats(&self) -> Option<HotKeyCacheStatsSnapshot> {
        self.hot_key_cache
            .as_ref()
            .map(|cache| cache.stats_snapshot())
    }

    /// Clears the hot-key cache.
    pub fn clear_hot_key_cache(&self) {
        if let Some(ref cache) = self.hot_key_cache {
            cache.clear();
            debug!(target: "neo", "PersistenceDataCache hot-key cache cleared");
        }
    }

    /// Clears the read cache.
    pub fn clear_read_cache(&self) {
        if let Some(ref cache) = self.read_cache {
            cache.clear();
            debug!(target: "neo", "PersistenceDataCache read cache cleared");
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
            for (key, trackable) in &state.dictionary {
                let Some(item) = overlay_item(trackable) else {
                    continue;
                };
                if !key_matches_prefix(key, prefix_bytes.as_deref()) {
                    continue;
                }
                overlays.insert(key.clone(), item);
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

            // With no contract id bounding the scan, C# seek order is the
            // serialized byte order (StorageKeyComparer); the BTreeMap's
            // numeric id order diverges for negative ids, so re-sort.
            if prefix_bytes.is_none() {
                let mut entries: Vec<_> = merged.into_iter().collect();
                entries.sort_by(|(a, _), (b, _)| a.compare_serialized(b));
                return match direction {
                    SeekDirection::Forward => Box::new(entries.into_iter()),
                    SeekDirection::Backward => Box::new(entries.into_iter().rev()),
                };
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

        // Unprefixed scans must follow C# byte order like the merged path.
        if prefix_bytes.is_none() {
            let mut entries: Vec<_> = base_items.into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.compare_serialized(b));
            return match direction {
                SeekDirection::Forward => Box::new(entries.into_iter()),
                SeekDirection::Backward => Box::new(entries.into_iter().rev()),
            };
        }

        match direction {
            SeekDirection::Forward => Box::new(base_items.into_iter()),
            SeekDirection::Backward => Box::new(base_items.into_iter().rev()),
        }
    }

    /// Finds items by key prefix, skipping the first `skip` entries in seek
    /// order (C# v3.11.2 `DataCache.Find(keyPrefix, direction, skip)`).
    pub fn find_with_skip(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
        skip: usize,
    ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
        Box::new(self.find(key_prefix, direction).skip(skip))
    }

    /// Returns the number of pending changes.
    pub fn pending_change_count(&self) -> usize {
        self.state
            .read()
            .dictionary
            .values()
            .filter(|t| {
                matches!(
                    t.state,
                    TrackState::Added | TrackState::Changed | TrackState::Deleted
                )
            })
            .count()
    }

    /// Returns true if there are any pending changes.
    pub fn has_pending_changes(&self) -> bool {
        self.state.read().dictionary.values().any(|t| {
            matches!(
                t.state,
                TrackState::Added | TrackState::Changed | TrackState::Deleted
            )
        })
    }

    /// Extracts all tracked changes as raw key-value pairs.
    pub fn extract_raw_changes(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let state = self.state.read();
        state
            .dictionary
            .iter()
            .filter_map(|(key, trackable)| match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    Some((key.to_array(), Some(trackable.item.to_value())))
                }
                TrackState::Deleted => Some((key.to_array(), None)),
                TrackState::None | TrackState::NotFound => None,
            })
            .collect()
    }
}

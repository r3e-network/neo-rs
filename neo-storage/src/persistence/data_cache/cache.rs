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

    /// Creates a new PersistenceDataCache with an optional backing store.
    pub fn new_with_store(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
    ) -> Self {
        Self::new_with_config(read_only, store_get, store_find, DataCacheConfig::default())
    }

    /// Creates a new PersistenceDataCache with configuration and optional backing store.
    pub fn new_with_config(
        read_only: bool,
        store_get: Option<Arc<StoreGetFn>>,
        store_find: Option<Arc<StoreFindFn>>,
        mut config: DataCacheConfig,
    ) -> Self {
        // Prefetch cannot be switched on by a configuration that asks for an
        // unbounded scan: the limits are clamped into the hard ceilings before
        // they are stored, whichever source produced them.
        if config.prefetch.enabled {
            config.prefetch = config.prefetch.sanitized();
        }
        let read_cache = if config.enable_read_cache {
            Some(Arc::new(ReadCache::new(config.read_cache_config)))
        } else {
            None
        };
        let hot_key_cache = config.enable_hot_key_cache.then(|| {
            Arc::new(HotKeyCache::new(
                config.hot_key_cache_capacity,
                HOT_KEY_CACHE_SHARDS,
            ))
        });

        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only,
            on_read: Arc::new(RwLock::new(Vec::new())),
            on_update: Arc::new(RwLock::new(Vec::new())),
            store_get,
            store_find,
            store_seek: None,
            ref_count: Arc::new(AtomicUsize::new(1)),
            read_cache,
            hot_key_cache,
            prefetch_window: RwLock::new(PrefetchWindow::new(prefetch_window_capacity(&config))),
            prefetch_metrics: Arc::new(PrefetchMetrics::new()),
            config,
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            commit_apply: None,
            generation_source: None,
            observed_generation: AtomicU64::new(0),
        }
    }

    /// Installs the bounded store scan used by speculative prefetch.
    ///
    /// A cache without this delegate never prefetches: falling back to the
    /// unbounded `store_find` would reintroduce exactly the eager
    /// materialisation prefetch is supposed to avoid.
    pub fn set_store_seek<F>(&mut self, seek: F)
    where
        F: Fn(
                Option<&StorageKey>,
                Option<&StorageKey>,
                SeekDirection,
                usize,
            ) -> Vec<(StorageKey, StorageItem)>
            + Send
            + Sync
            + 'static,
    {
        self.store_seek = Some(Arc::new(seek));
    }

    /// Returns the prefetch counters as of this call.
    #[must_use]
    pub fn prefetch_metrics(&self) -> PrefetchMetricsSnapshot {
        self.prefetch_metrics.snapshot()
    }

    fn prefetch_window_capacity(&self) -> usize {
        prefetch_window_capacity(&self.config)
    }

    /// Binds this cache to a mutable backing store's version probe.
    ///
    /// Caches created over an immutable snapshot must not call this: a snapshot
    /// view never changes underneath its readers. Latest-state caches do change,
    /// so every read validates that no external commit happened since the
    /// previous one and drops mirrors and LRU entries when it did.
    pub fn set_generation_source<F>(&mut self, source: F)
    where
        F: Fn() -> u64 + Send + Sync + 'static,
    {
        let observed = source();
        self.generation_source = Some(Arc::new(source));
        self.observed_generation.store(observed, Ordering::Release);
    }

    /// Discards cached reads after the backing store changed outside this cache.
    ///
    /// Only `TrackState::None` mirrors are dropped: `Added`/`Changed`/`Deleted`
    /// entries are pending writes owned by this cache and must survive.
    #[inline]
    fn refresh_if_store_changed(&self) {
        let Some(source) = &self.generation_source else {
            return;
        };
        let current = source();
        if current == self.observed_generation.load(Ordering::Acquire) {
            return;
        }

        {
            let mut state = self.state.write();
            state
                .dictionary
                .retain(|_, trackable| !matches!(trackable.state, TrackState::None));
        }
        if let Some(ref cache) = self.read_cache {
            cache.clear();
        }
        if let Some(ref cache) = self.hot_key_cache {
            cache.clear();
        }
        self.observed_generation.store(current, Ordering::Release);
        debug!(
            target: "neo",
            generation = current,
            "PersistenceDataCache dropped stale read state after an external commit"
        );
    }

    /// Returns true if PersistenceDataCache is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Returns whether this cache has an authoritative persistent read view.
    ///
    /// A `None` result from [`Self::get`] is then a confirmed absence. Callers
    /// must retain a separate fallback only for detached caches without one.
    #[must_use]
    pub fn has_backing_store(&self) -> bool {
        self.store_get.is_some()
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

        // Each overlay has an independent lifetime. A sibling may commit while this
        // overlay is still alive, so neither a private LRU nor TrackState::None read
        // mirrors may retain values with no opportunity for the parent to invalidate
        // them. Keep read caching in the shared parent; writes remain tracked locally.
        let overlay_config = DataCacheConfig {
            track_reads_in_write_cache: false,
            enable_read_cache: false,
            enable_hot_key_cache: false,
            prefetch: PrefetchConfig::disabled(),
            ..self.config
        };
        let mut overlay =
            Self::new_with_config(false, Some(store_get), Some(store_find), overlay_config);
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

    /// Drains tracked changes from a child cache and merges them into this cache without cloning.
    pub fn merge_from_child(&self, child: &PersistenceDataCache) {
        let drained: Vec<(StorageKey, Trackable)> = {
            let mut child_state = child.state.write();
            child_state.dictionary.drain().collect()
        };
        for (key, trackable) in drained {
            log_watched_storage_event(
                "merge",
                "merge_tracked_items",
                &key,
                None,
                Some(trackable.state),
                Some(&trackable.item),
            );
            match trackable.state {
                TrackState::Added => self.add(key, trackable.item),
                TrackState::Changed => self.update(key, trackable.item),
                TrackState::Deleted => self.delete(&key),
                TrackState::None | TrackState::NotFound => {}
            }
        }
    }

    /// Creates an isolated writable fork backed by the same underlying store.
    ///
    /// The fork owns its write dictionary *and* its read cache. Sharing the
    /// parent's LRU would let speculative fork writes (and speculative reads
    /// that the parent can never invalidate) leak into the parent's view.
    pub fn fork_isolated(&self) -> Self {
        let state = self.state.read();
        let cloned_state = InnerState {
            dictionary: state.dictionary.clone(),
        };
        drop(state);

        let read_cache = self
            .config
            .enable_read_cache
            .then(|| Arc::new(ReadCache::new(self.config.read_cache_config)));
        // A fork owns a private hot-key cache: its speculative fills are
        // discarded together with the fork, never observed by the parent.
        let hot_key_cache = self.config.enable_hot_key_cache.then(|| {
            Arc::new(HotKeyCache::new(
                self.config.hot_key_cache_capacity,
                HOT_KEY_CACHE_SHARDS,
            ))
        });

        Self {
            state: Arc::new(RwLock::new(cloned_state)),
            read_only: self.read_only,
            on_read: Arc::clone(&self.on_read),
            on_update: Arc::clone(&self.on_update),
            store_get: self.store_get.as_ref().map(Arc::clone),
            store_find: self.store_find.as_ref().map(Arc::clone),
            store_seek: self.store_seek.as_ref().map(Arc::clone),
            ref_count: Arc::new(AtomicUsize::new(1)),
            read_cache,
            hot_key_cache,
            // A fork owns a private, short-lived read cache: speculative fills
            // would be discarded with it, so prefetch stays off in forks.
            config: DataCacheConfig {
                prefetch: PrefetchConfig::disabled(),
                ..self.config
            },
            pattern_tracker: RwLock::new(AccessPatternTracker::new()),
            access_seq: AtomicU64::new(0),
            prefetch_window: RwLock::new(PrefetchWindow::new(self.prefetch_window_capacity())),
            prefetch_metrics: Arc::clone(&self.prefetch_metrics),
            commit_apply: None,
            generation_source: self.generation_source.as_ref().map(Arc::clone),
            observed_generation: AtomicU64::new(self.observed_generation.load(Ordering::Acquire)),
        }
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

    /// Adds an item to the cache.
    pub fn add(&self, key: StorageKey, value: StorageItem) {
        if self.try_add(key, value).is_err() {
            warn!("attempted to add to read-only PersistenceDataCache");
        }
    }

    fn add_writable(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(&key);
        }
        if let Some(ref cache) = self.read_cache {
            cache.remove(&key);
        }

        let handlers = self.on_update.read();
        if handlers.is_empty() {
            drop(handlers);
            self.apply_add(key, value)?;
        } else {
            let key_clone = key.clone();
            let value_clone = value.clone();
            self.apply_add(key, value)?;
            for handler in handlers.iter() {
                handler(self, &key_clone, &value_clone);
            }
        }
        Ok(())
    }

    fn apply_add(&self, key: StorageKey, value: StorageItem) -> DataCacheResult {
        use std::collections::hash_map::Entry;
        let mut state = self.state.write();
        match state.dictionary.entry(key) {
            Entry::Occupied(mut occupied) => {
                let prev_state = occupied.get().state;
                let new_state = match prev_state {
                    TrackState::Deleted => TrackState::Changed,
                    TrackState::NotFound => TrackState::Added,
                    TrackState::None | TrackState::Added | TrackState::Changed => {
                        return Err(DataCacheError::InvalidState(format!(
                            "Add is invalid for existing state {prev_state:?}"
                        )));
                    }
                };
                log_watched_storage_event(
                    "add",
                    "apply_add",
                    occupied.key(),
                    Some(prev_state),
                    Some(new_state),
                    Some(&value),
                );
                let entry = occupied.get_mut();
                entry.item = value;
                entry.state = new_state;
            }
            Entry::Vacant(vacant) => {
                log_watched_storage_event(
                    "add",
                    "apply_add",
                    vacant.key(),
                    Some(TrackState::NotFound),
                    Some(TrackState::Added),
                    Some(&value),
                );
                vacant.insert(Trackable::new(value, TrackState::Added));
            }
        }
        Ok(())
    }

    /// Updates an item in the cache.
    pub fn update(&self, key: StorageKey, value: StorageItem) {
        if self.try_update(key, value).is_err() {
            warn!("attempted to update read-only PersistenceDataCache");
        }
    }

    fn update_writable(&self, key: StorageKey, value: StorageItem) {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(&key);
        }
        if let Some(ref cache) = self.read_cache {
            let size = value.value_bytes().len() + std::mem::size_of::<StorageKey>();
            cache.put(key.clone(), value.clone(), size);
        }

        let handlers = self.on_update.read();
        if handlers.is_empty() {
            drop(handlers);
            self.apply_update(key, value);
        } else {
            let key_clone = key.clone();
            let value_clone = value.clone();
            self.apply_update(key, value);
            for handler in handlers.iter() {
                handler(self, &key_clone, &value_clone);
            }
        }
    }

    fn apply_update(&self, key: StorageKey, value: StorageItem) {
        use std::collections::hash_map::Entry;
        let mut state = self.state.write();
        match state.dictionary.entry(key) {
            Entry::Occupied(mut occupied) => {
                let prev_state = occupied.get().state;
                // A tombstone that is re-populated modifies pre-existing data:
                // tracking it as `Added` would make a later delete cancel the
                // entry entirely and lose the tombstone.
                let new_state = match prev_state {
                    TrackState::Added => TrackState::Added,
                    TrackState::Changed | TrackState::None | TrackState::Deleted => {
                        TrackState::Changed
                    }
                    TrackState::NotFound => TrackState::Changed,
                };
                log_watched_storage_event(
                    "update",
                    "apply_update",
                    occupied.key(),
                    Some(prev_state),
                    Some(new_state),
                    Some(&value),
                );
                let entry = occupied.get_mut();
                entry.item = value;
                entry.state = new_state;
            }
            Entry::Vacant(vacant) => {
                log_watched_storage_event(
                    "update",
                    "apply_update",
                    vacant.key(),
                    Some(TrackState::NotFound),
                    Some(TrackState::Changed),
                    Some(&value),
                );
                vacant.insert(Trackable::new(value, TrackState::Changed));
            }
        }
    }

    /// Deletes an item from the cache.
    pub fn delete(&self, key: &StorageKey) {
        if self.try_delete(key).is_err() {
            warn!("attempted to delete from read-only PersistenceDataCache");
        }
    }

    fn delete_writable(&self, key: &StorageKey) {
        if let Some(ref cache) = self.hot_key_cache {
            cache.invalidate(key);
        }
        if let Some(ref cache) = self.read_cache {
            cache.remove(key);
        }

        self.apply_delete(key);
    }

    fn apply_delete(&self, key: &StorageKey) {
        let prev_state = {
            let state = self.state.read();
            state
                .dictionary
                .get(key)
                .map_or(TrackState::NotFound, |t| t.state)
        };

        match prev_state {
            TrackState::Added => {
                let mut state = self.state.write();
                state.dictionary.remove(key);
                log_watched_storage_event(
                    "delete",
                    "apply_delete_added",
                    key,
                    Some(prev_state),
                    Some(TrackState::NotFound),
                    None,
                );
            }
            TrackState::Changed | TrackState::None => {
                let mut state = self.state.write();
                if let Some(entry) = state.dictionary.get_mut(key) {
                    entry.item = StorageItem::default();
                    entry.state = TrackState::Deleted;
                } else {
                    state.dictionary.insert(
                        key.clone(),
                        Trackable::new(StorageItem::default(), TrackState::Deleted),
                    );
                }
                log_watched_storage_event(
                    "delete",
                    "apply_delete_tracked",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    None,
                );
            }
            TrackState::Deleted => {
                log_watched_storage_event(
                    "delete",
                    "apply_delete_already_deleted",
                    key,
                    Some(prev_state),
                    Some(TrackState::Deleted),
                    None,
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
                    .map_or(TrackState::NotFound, |t| t.state);
                match current_state {
                    TrackState::Added => {
                        state.dictionary.remove(key);
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
        state
            .dictionary
            .retain(|_, trackable| match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    trackable.state = TrackState::None;
                    true
                }
                TrackState::Deleted => false,
                TrackState::None | TrackState::NotFound => true,
            });
    }

    /// Resets the cache for reuse.
    pub fn reset(&self) {
        let mut state = self.state.write();
        state.dictionary.clear();
        drop(state);
        if let Some(ref cache) = self.hot_key_cache {
            cache.clear();
        }
        if self.config.prefetch.enabled {
            *self.pattern_tracker.write() = AccessPatternTracker::new();
            self.prefetch_window.write().clear();
        }
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

    /// Writes all modified entries directly to a byte store writer without intermediate cloning.
    pub fn apply_tracked_to_writer<W>(&self, writer: &mut W) -> crate::error::StorageResult<bool>
    where
        W: crate::persistence::write_store::WriteStore<Vec<u8>, Vec<u8>> + ?Sized,
    {
        let state = self.state.read();
        let mut any_written = false;
        for (key, trackable) in &state.dictionary {
            match trackable.state {
                TrackState::Added | TrackState::Changed => {
                    writer.put(key.to_array(), trackable.item.to_value())?;
                    any_written = true;
                }
                TrackState::Deleted => {
                    writer.delete(key.to_array())?;
                    any_written = true;
                }
                TrackState::None | TrackState::NotFound => {}
            }
        }
        Ok(any_written)
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

    /// Records an access pattern and returns a bounded scan plan when one is due.
    pub(super) fn record_access_pattern(&self, key: &StorageKey) -> Option<PrefetchScanPlan> {
        let seq = self.access_seq.fetch_add(1, Ordering::Relaxed);
        self.pattern_tracker
            .write()
            .record_access(key, seq, &self.config.prefetch)
    }

    /// Gets the current detected prefetch pattern.
    pub fn current_prefetch_pattern(&self) -> PrefetchPattern {
        self.pattern_tracker
            .read()
            .current_pattern(self.config.prefetch.confidence_threshold)
    }

    /// Checks if a key is inside the prefetch window.
    pub fn is_recently_prefetched(&self, key: &StorageKey) -> bool {
        self.prefetch_window.read().contains(key)
    }

    /// Clears the prefetch window.
    pub fn clear_prefetch_window(&self) {
        self.prefetch_window.write().clear();
    }

    #[inline]
    fn prefetch_active(&self) -> bool {
        self.config.prefetch.is_active() && self.read_cache.is_some() && self.store_seek.is_some()
    }

    /// Removes `key` from the window, reporting whether prefetch put it there.
    fn take_prefetched(&self, key: &StorageKey) -> bool {
        if !self.config.prefetch.enabled {
            return false;
        }
        self.prefetch_window.write().take(key)
    }

    /// Returns true when this cache owns pending state for `key`.
    ///
    /// Any dictionary entry — a write, a delete, a tombstone or a read mirror —
    /// belongs to the overlay and must never be replaced by a speculative value.
    fn has_local_state(&self, key: &StorageKey) -> bool {
        self.state.read().dictionary.contains_key(key)
    }

    /// Runs one bounded speculative scan described by `plan`.
    ///
    /// The scan is confined to `plan.prefix`, reads at most
    /// `limits.max_scan_keys` rows, admits at most `limits.max_admit_keys` keys
    /// and `limits.max_admit_bytes` value bytes, skips anything this cache owns
    /// locally, and tags the batch with the read-cache generation captured
    /// before the scan so a concurrent refresh can reject it.
    fn run_prefetch(&self, plan: PrefetchScanPlan) {
        let (Some(cache), Some(seek)) = (self.read_cache.as_ref(), self.store_seek.as_ref()) else {
            return;
        };
        let config = &self.config.prefetch;
        let admit_target = config.admit_target();
        if admit_target == 0 || config.limits.max_scan_keys == 0 {
            self.prefetch_metrics.record_declined();
            return;
        }

        // Snapshot every clock the batch will be validated against before the
        // backing store is touched.
        let source_generation = cache.generation();
        let store_generation = self.observed_generation.load(Ordering::Acquire);

        self.prefetch_metrics.record_request();
        let scanned = seek(
            Some(&plan.prefix),
            Some(&plan.start),
            plan.direction,
            config.limits.max_scan_keys,
        );
        self.prefetch_metrics.record_scan(scanned.len());
        if scanned.is_empty() {
            self.prefetch_metrics.record_declined();
            return;
        }

        // A commit that landed mid-scan makes every row suspect: drop the batch
        // instead of admitting values the refresh has already invalidated.
        if let Some(source) = &self.generation_source
            && source() != store_generation
        {
            self.prefetch_metrics.record_canceled();
            self.clear_prefetch_window();
            return;
        }

        let prefix_bytes = plan.prefix.to_array();
        let mut items: Vec<(StorageKey, StorageItem, usize)> = Vec::with_capacity(admit_target);
        let mut admitted_bytes = 0usize;
        let mut declined = 0u64;
        let mut scanned_kept = 0usize;
        for (key, item) in scanned {
            scanned_kept += 1;
            if items.len() >= admit_target {
                break;
            }
            // Defensive: never admit a row outside the namespace the plan named.
            if !key.as_bytes().starts_with(&prefix_bytes) {
                break;
            }
            if self.is_recently_prefetched(&key) || self.has_local_state(&key) {
                declined += 1;
                continue;
            }
            let size = item.value_bytes().len() + std::mem::size_of::<StorageKey>();
            if admitted_bytes.saturating_add(size) > config.limits.max_admit_bytes {
                declined += 1;
                break;
            }
            admitted_bytes = admitted_bytes.saturating_add(size);
            items.push((key, item, size));
        }

        if items.is_empty() {
            self.prefetch_metrics.record_declined();
            return;
        }

        {
            let mut window = self.prefetch_window.write();
            for (key, _, _) in &items {
                window.record(key.clone());
            }
        }

        let admission = cache.put_batch_with_generation(items, source_generation);
        self.prefetch_metrics.record_admission(
            admission.admitted as u64,
            admission.stale_skips as u64,
            admission.bytes as u64,
        );
        for _ in 0..declined.saturating_add(admission.rejected as u64) {
            self.prefetch_metrics.record_declined();
        }
        trace!(
            target: "neo",
            direction = ?plan.direction,
            scanned = scanned_kept,
            admitted = admission.admitted,
            "prefetched sequential keys"
        );
    }

    /// Pre-fetches items into the read cache.
    ///
    /// Public entry point for callers that already hold speculative values. The
    /// batch is tagged with the current read-cache generation and never replaces
    /// a resident entry, matching the internal prefetch path.
    pub fn prefetch(&self, items: Vec<(StorageKey, StorageItem)>) {
        if let Some(ref cache) = self.read_cache {
            let generation = cache.generation();
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = v.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    (k, v, size)
                })
                .collect();
            let admission = cache.put_batch_with_generation(cache_items, generation);
            self.prefetch_metrics.record_admission(
                admission.admitted as u64,
                admission.stale_skips as u64,
                admission.bytes as u64,
            );
        }
    }
}

impl ReadOnlyStore for PersistenceDataCache {
    /// Overlay-first bounded scan.
    ///
    /// Backing rows are read through the bounded seek when one is installed, so
    /// the scan never materialises a prefix range larger than `limit`. Local
    /// writes and tombstones are then applied on top, which keeps this call
    /// consistent with [`Self::find`] without multiplying the read cost.
    fn seek_limited(
        &self,
        key_prefix: Option<&StorageKey>,
        start: Option<&StorageKey>,
        direction: SeekDirection,
        limit: usize,
    ) -> Vec<(StorageKey, StorageItem)> {
        if limit == 0 {
            return Vec::new();
        }

        let prefix_bytes = key_prefix.map(|key| key.as_bytes().into_owned());
        let inside_cursor = |key: &StorageKey| match direction {
            SeekDirection::Forward => start.is_none_or(|cursor| *key > *cursor),
            SeekDirection::Backward => start.is_none_or(|cursor| *key < *cursor),
        };

        let backing = match &self.store_seek {
            Some(seek) => seek(key_prefix, start, direction, limit),
            None => match &self.store_find {
                Some(find) => find(key_prefix, direction)
                    .into_iter()
                    .filter(|(key, _)| {
                        key_matches_prefix(key, prefix_bytes.as_deref()) && inside_cursor(key)
                    })
                    .take(limit)
                    .collect(),
                None => Vec::new(),
            },
        };

        let mut merged: BTreeMap<StorageKey, StorageItem> = backing.into_iter().collect();
        {
            let state = self.state.read();
            for (key, trackable) in &state.dictionary {
                if !key_matches_prefix(key, prefix_bytes.as_deref()) || !inside_cursor(key) {
                    continue;
                }
                match overlay_item(trackable) {
                    Some(Some(item)) => {
                        merged.insert(key.clone(), item);
                    }
                    Some(None) => {
                        merged.remove(key);
                    }
                    None => {}
                }
            }
        }

        match direction {
            SeekDirection::Forward => merged.into_iter().take(limit).collect(),
            SeekDirection::Backward => merged.into_iter().rev().take(limit).collect(),
        }
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for PersistenceDataCache {
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
mod tests {
    use super::*;

    fn item(value: &[u8]) -> StorageItem {
        StorageItem::from_bytes(value.to_vec())
    }

    fn state_of(cache: &PersistenceDataCache, key: &StorageKey) -> Option<TrackState> {
        cache
            .state
            .read()
            .dictionary
            .get(key)
            .map(|entry| entry.state)
    }

    fn untracked_read_config() -> DataCacheConfig {
        DataCacheConfig {
            track_reads_in_write_cache: false,
            ..DataCacheConfig::default()
        }
    }

    #[test]
    fn delete_update_delete_preserves_the_tombstone() {
        let cache = PersistenceDataCache::new(false);
        let key = StorageKey::new(1, b"k".to_vec());

        cache.add(key.clone(), item(b"v1"));
        cache.commit();
        cache.delete(&key);
        assert_eq!(state_of(&cache, &key), Some(TrackState::Deleted));

        // Re-populating a tombstone modifies pre-existing data, so the entry
        // must remain tracked as a modification rather than a fresh add.
        cache.update(key.clone(), item(b"v2"));
        assert_eq!(state_of(&cache, &key), Some(TrackState::Changed));

        // The second delete must leave a real tombstone behind instead of
        // cancelling the entry (which would resurrect the old value on commit).
        cache.delete(&key);
        assert_eq!(state_of(&cache, &key), Some(TrackState::Deleted));
        assert_eq!(
            cache.extract_raw_changes(),
            vec![(key.to_array(), None)],
            "a re-deleted key must still emit a deletion"
        );
    }

    #[test]
    fn isolated_fork_does_not_share_the_parent_read_cache() {
        let key = StorageKey::new(1, b"shared".to_vec());
        let store_get: Arc<StoreGetFn> =
            Arc::new(|_key: &StorageKey| Some(StorageItem::from_bytes(b"parent".to_vec())));
        let parent = PersistenceDataCache::new_with_config(
            false,
            Some(store_get),
            None,
            untracked_read_config(),
        );
        assert_eq!(parent.get(&key).unwrap().to_value(), b"parent".to_vec());

        let fork = parent.fork_isolated();
        fork.update(key.clone(), item(b"fork"));

        assert_eq!(fork.get(&key).unwrap().to_value(), b"fork".to_vec());
        // A speculative fork write must never become visible to the parent.
        assert_eq!(parent.get(&key).unwrap().to_value(), b"parent".to_vec());
    }

    #[test]
    fn external_commit_generation_drops_stale_reads() {
        let backing = Arc::new(parking_lot::Mutex::new(b"v1".to_vec()));
        let backing_get = Arc::clone(&backing);
        let store_get: Arc<StoreGetFn> = Arc::new(move |_key: &StorageKey| {
            Some(StorageItem::from_bytes(backing_get.lock().clone()))
        });
        let mut cache = PersistenceDataCache::new_with_config(
            false,
            Some(store_get),
            None,
            DataCacheConfig::default(),
        );
        let generation = Arc::new(AtomicU64::new(0));
        let generation_probe = Arc::clone(&generation);
        cache.set_generation_source(move || generation_probe.load(Ordering::Acquire));

        let key = StorageKey::new(9, b"k".to_vec());
        let pending = StorageKey::new(9, b"pending".to_vec());
        assert_eq!(cache.get(&key).unwrap().to_value(), b"v1".to_vec());
        cache.try_add(pending.clone(), item(b"pending")).unwrap();

        // A sibling writer commits a new value and advances the store version.
        *backing.lock() = b"v2".to_vec();
        generation.fetch_add(1, Ordering::AcqRel);

        assert_eq!(cache.get(&key).unwrap().to_value(), b"v2".to_vec());
        // Pending writes are owned by this cache and survive the refresh.
        assert_eq!(cache.get(&pending).unwrap().to_value(), b"pending".to_vec());
        assert_eq!(cache.pending_change_count(), 1);
    }
}

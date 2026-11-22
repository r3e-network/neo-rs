use super::*;
impl PersistenceDataCache {
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

    pub(super) fn prefetch_window_capacity(&self) -> usize {
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
    pub(super) fn refresh_if_store_changed(&self) {
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
}

use super::storage_watch::log_watched_storage_event;
use super::trackable::{DataCacheConfig, DataCacheError, DataCacheResult, InnerState, Trackable};
use crate::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::store::{RawOverlaySink, RawOverlaySource};
use crate::types::{StorageItem, StorageKey, TrackState};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tracing::warn;

/// Statically dispatched read source used for cache misses and prefix scans.
///
/// Implementations are concrete types carried by [`DataCache`]. The cache does
/// not erase stores behind callbacks or trait objects, so homogeneous node
/// pipelines retain their backend type through execution.
pub trait CacheRead: Clone + Send + Sync + 'static {
    /// Reads one storage entry from the backing source.
    fn get(&self, key: &StorageKey) -> Option<StorageItem>;

    /// Reads a prefix range from the backing source.
    ///
    /// `None` means that this cache has no external source. That distinction
    /// lets an in-memory cache retain committed entries from its own dictionary.
    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>>;
}

/// Backing type for standalone in-memory caches.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyCacheBacking;

impl CacheRead for EmptyCacheBacking {
    fn get(&self, _key: &StorageKey) -> Option<StorageItem> {
        None
    }

    fn find(
        &self,
        _prefix: Option<&StorageKey>,
        _direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>> {
        None
    }
}

#[derive(Clone)]
enum CacheBacking<B> {
    Source(B),
    Parent(Arc<DataCache<B>>),
}

impl<B: CacheRead> CacheBacking<B> {
    fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self {
            Self::Source(source) => source.get(key),
            Self::Parent(parent) => parent.get(key),
        }
    }

    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>> {
        match self {
            Self::Source(source) => source.find(prefix, direction),
            Self::Parent(parent) => Some(parent.find(prefix, direction).collect()),
        }
    }

    fn parent(&self) -> Option<&DataCache<B>> {
        match self {
            Self::Parent(parent) => Some(parent.as_ref()),
            Self::Source(_) => None,
        }
    }
}

/// Represents a cache for the underlying storage of the NEO blockchain.
pub struct DataCache<B = EmptyCacheBacking> {
    /// Shared state with CoW optimization
    state: Arc<RwLock<InnerState>>,
    /// Read-only flag (determines if changes are tracked)
    read_only: bool,
    /// Optional backing source for cache misses and cloned-cache commits.
    backing: CacheBacking<B>,
    /// Strong count for CoW detection
    ref_count: Arc<AtomicUsize>,
    /// Configuration
    config: DataCacheConfig,
    /// Counts parent write passes used by cloned-cache merges in tests.
    #[cfg(test)]
    merge_write_passes: Arc<AtomicUsize>,
    /// Counts tracked-item snapshot materializations in tests.
    #[cfg(test)]
    tracked_items_calls: Arc<AtomicUsize>,
    /// Counts zero-copy tracked-item visits in tests.
    #[cfg(test)]
    tracked_item_visit_calls: Arc<AtomicUsize>,
}

impl<B: CacheRead> RawOverlaySource for &DataCache<B> {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        DataCache::visit_raw_changes(self, |key, value| sink.visit(key, value));
    }
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

impl<B: CacheRead> Clone for DataCache<B> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            read_only: self.read_only,
            backing: self.backing.clone(),
            ref_count: Arc::clone(&self.ref_count),
            config: self.config,
            #[cfg(test)]
            merge_write_passes: Arc::clone(&self.merge_write_passes),
            #[cfg(test)]
            tracked_items_calls: Arc::clone(&self.tracked_items_calls),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::clone(&self.tracked_item_visit_calls),
        }
    }
}

impl DataCache<EmptyCacheBacking> {
    /// Creates a new DataCache.
    pub fn new(read_only: bool) -> Self {
        Self::with_config(read_only, DataCacheConfig::default())
    }

    /// Creates a new DataCache with configuration.
    pub fn with_config(read_only: bool, config: DataCacheConfig) -> Self {
        Self::with_backing(read_only, EmptyCacheBacking, config)
    }
}

impl<B: CacheRead> DataCache<B> {
    /// Creates a cache over a concrete backing reader.
    pub fn with_backing(read_only: bool, backing: B, config: DataCacheConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only,
            backing: CacheBacking::Source(backing),
            ref_count: Arc::new(AtomicUsize::new(1)),
            config,
            #[cfg(test)]
            merge_write_passes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_items_calls: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::new(AtomicUsize::new(0)),
        }
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

    /// Returns true if DataCache is read-only.
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Creates a cloned overlay cache that uses this cache as the backing store.
    pub fn clone_cache(&self) -> Self {
        let parent = Arc::new(self.clone());
        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only: false,
            backing: CacheBacking::Parent(parent),
            ref_count: Arc::new(AtomicUsize::new(1)),
            config: self.config,
            #[cfg(test)]
            merge_write_passes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_items_calls: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn merge_tracked_items_from(&self, source: &DataCache<B>) {
        self.merge_tracked_items_without_update_callbacks(source);
    }

    fn merge_tracked_items_without_update_callbacks(&self, source: &DataCache<B>) {
        let source_state = source.state.read();
        if source_state.change_set.is_empty() {
            return;
        }

        let mut state = self.state.write();
        #[cfg(test)]
        self.merge_write_passes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        for key in &source_state.change_set {
            let Some(trackable) = source_state.dictionary.get(key) else {
                continue;
            };
            log_watched_storage_event(
                "merge",
                "merge_tracked_items_fast",
                key,
                None,
                Some(trackable.state),
                Some(&trackable.item),
            );
            match trackable.state {
                TrackState::Added => {
                    let prev_state = state
                        .dictionary
                        .get(key)
                        .map(|t| t.state)
                        .unwrap_or(TrackState::NotFound);
                    let new_state = match prev_state {
                        TrackState::Deleted => TrackState::Changed,
                        TrackState::NotFound => TrackState::Added,
                        TrackState::Added | TrackState::Changed | TrackState::None => {
                            continue;
                        }
                    };
                    state.dictionary.insert(
                        key.clone(),
                        Trackable::new(trackable.item.clone(), new_state),
                    );
                    state.change_set.insert(key.clone());
                }
                TrackState::Changed => {
                    let prev_state = state
                        .dictionary
                        .get(key)
                        .map(|t| t.state)
                        .unwrap_or(TrackState::NotFound);
                    let new_state = match prev_state {
                        TrackState::Added => TrackState::Added,
                        TrackState::Changed | TrackState::None => TrackState::Changed,
                        TrackState::Deleted => TrackState::Changed,
                        TrackState::NotFound => TrackState::Changed,
                    };
                    state.dictionary.insert(
                        key.clone(),
                        Trackable::new(trackable.item.clone(), new_state),
                    );
                    state.change_set.insert(key.clone());
                }
                TrackState::Deleted => match state
                    .dictionary
                    .get(key)
                    .map(|trackable| trackable.state)
                    .unwrap_or(TrackState::NotFound)
                {
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
                    TrackState::NotFound => {
                        // C# `parent.Delete(key)` (invoked by `ClonedCache.Commit`
                        // via `DeleteInternal`) records a tombstone for a key absent
                        // from the parent dictionary ONLY if it exists in the parent's
                        // underlying store (`TryGetInternalWrapper != null`); otherwise
                        // it is a no-op. Matching that here keeps the fast merge path
                        // from injecting a spurious `Deleted` into the change set (which
                        // would perturb the MPT state-root diff) — the same store check
                        // the per-item slow path (`apply_delete`) already performs.
                        let exists_in_store = self.backing.get(key).is_some();
                        if exists_in_store {
                            state.dictionary.insert(
                                key.clone(),
                                Trackable::new(StorageItem::default(), TrackState::Deleted),
                            );
                            state.change_set.insert(key.clone());
                        }
                    }
                    TrackState::Deleted => {}
                },
                TrackState::None | TrackState::NotFound => {}
            }
        }
    }

    /// Merges tracked changes from another cache into this one.
    pub fn merge_tracked_items(&self, items: &[(StorageKey, Trackable)]) {
        for (key, trackable) in items {
            self.merge_tracked_item(key, trackable);
        }
    }

    fn merge_tracked_item(&self, key: &StorageKey, trackable: &Trackable) {
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
            backing: self.backing.clone(),
            ref_count: Arc::new(AtomicUsize::new(1)),
            config: self.config,
            #[cfg(test)]
            merge_write_passes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_items_calls: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::new(AtomicUsize::new(0)),
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
        if let Some(item) = self.backing.get(key) {
            self.track_in_write_cache(key, &item);

            log_watched_storage_event("get", "store_get_hit", key, None, None, Some(&item));
            return Some(item);
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
        self.apply_add(&key, value)
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
        self.apply_update(&key, value);
    }

    fn apply_update(&self, key: &StorageKey, value: StorageItem) {
        let mut state = self.state.write();
        let prev_state = state
            .dictionary
            .get(key)
            .map(|t| t.state)
            .unwrap_or(TrackState::NotFound);
        // Re-writing a previously `Deleted` entry must restore it as
        // `Changed` — the key still exists in the backing store, so the
        // net effect is a value change, exactly as C#
        // `DataCache.GetAndChange` does (Persistence/DataCache.cs:
        // `Deleted -> Changed`). The prior Rust behaviour used `Added`
        // here, which makes a delete-then-recreate within one commit
        // cycle (e.g. a GAS balance burned to exactly zero then
        // re-credited in the same block) merge via `add()` into a parent
        // that read-cached the key as `None`; that fails `apply_add`
        // (InvalidState) and is silently swallowed, leaving the stale
        // pre-deletion value in the store. `NotFound` stays `Changed`
        // (rather than C#'s `Added`): it is consensus-identical for the
        // committed value but always merges via `update()`, avoiding the
        // same `add()`-into-`None` swallow for blind writes (`Storage.Put`
        // on an unread, store-present key).
        let new_state = match prev_state {
            TrackState::Added => TrackState::Added,
            TrackState::Changed | TrackState::None => TrackState::Changed,
            TrackState::Deleted => TrackState::Changed,
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
                let store_item = self.backing.get(key);
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
        if let Some(parent) = self.backing.parent() {
            if self.has_pending_changes() {
                parent.merge_tracked_items_from(self);
            }
        }

        let mut state = self.state.write();
        let keys: Vec<StorageKey> = state.change_set.drain().collect();
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
        #[cfg(test)]
        self.tracked_items_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

    /// Visits tracked items without cloning keys/items before the visitor needs them.
    pub fn visit_tracked_items<F>(&self, mut visit: F)
    where
        F: FnMut(&StorageKey, &Trackable),
    {
        #[cfg(test)]
        self.tracked_item_visit_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = self.state.read();
        for key in &state.change_set {
            if let Some(trackable) = state.dictionary.get(key) {
                visit(key, trackable);
            }
        }
    }

    /// Visits tracked items in byte-key order. This is the commit-facing path
    /// for storage engines that benefit from sorted B+tree/LSM writes.
    pub fn visit_tracked_items_sorted<F>(&self, mut visit: F)
    where
        F: FnMut(&StorageKey, &Trackable),
    {
        #[cfg(test)]
        self.tracked_item_visit_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let state = self.state.read();
        let mut keys = state.change_set.iter().collect::<Vec<_>>();
        keys.sort_unstable();
        for key in keys {
            if let Some(trackable) = state.dictionary.get(key) {
                visit(key, trackable);
            }
        }
    }

    /// Visits tracked changes as raw key/value byte slices without building an
    /// intermediate overlay vector. Entries are emitted in byte-key order so
    /// storage engines can turn random overlays into ordered batches.
    /// Cache-backed values may materialise into a short-lived owned buffer for
    /// the duration of the callback.
    pub fn visit_raw_changes<F>(&self, mut visit: F)
    where
        F: FnMut(&[u8], Option<&[u8]>),
    {
        self.visit_tracked_items_sorted(|key, trackable| match trackable.state {
            TrackState::Added | TrackState::Changed => {
                let key_bytes = key.as_bytes();
                let value = trackable.item.value_bytes();
                visit(key_bytes.as_ref(), Some(value.as_ref()));
            }
            TrackState::Deleted => {
                let key_bytes = key.as_bytes();
                visit(key_bytes.as_ref(), None);
            }
            TrackState::None | TrackState::NotFound => {}
        });
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
    ) -> std::vec::IntoIter<(StorageKey, StorageItem)> {
        let prefix_bytes = key_prefix.map(|k| k.as_bytes().into_owned());

        if let Some(backing_entries) = self.backing.find(key_prefix, direction) {
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
                return backing_entries
                    .into_iter()
                    .filter(move |(key, _)| key_matches_prefix(key, prefix_bytes.as_deref()))
                    .collect::<Vec<_>>()
                    .into_iter();
            }

            let mut merged = BTreeMap::new();
            for (key, item) in backing_entries {
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

            let mut entries = merged.into_iter().collect::<Vec<_>>();
            if direction == SeekDirection::Backward {
                entries.reverse();
            }
            return entries.into_iter();
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

        let mut entries = base_items.into_iter().collect::<Vec<_>>();
        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        entries.into_iter()
    }

    /// Returns the number of pending changes.
    pub fn pending_change_count(&self) -> usize {
        self.state.read().change_set.len()
    }

    #[cfg(test)]
    pub(crate) fn merge_write_pass_count(&self) -> usize {
        self.merge_write_passes
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn tracked_items_call_count(&self) -> usize {
        self.tracked_items_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn tracked_item_visit_call_count(&self) -> usize {
        self.tracked_item_visit_calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Returns true if there are any pending changes.
    pub fn has_pending_changes(&self) -> bool {
        !self.state.read().change_set.is_empty()
    }

    /// Extracts all tracked changes as raw key-value pairs.
    pub fn extract_raw_changes(&self) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        let mut changes = Vec::new();
        self.visit_raw_changes(|key, value| {
            changes.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        });
        changes
    }
}

impl<B: CacheRead> ReadOnlyStore for DataCache<B> {}

impl<B: CacheRead> ReadOnlyStoreGeneric<StorageKey, StorageItem> for DataCache<B> {
    type FindIterator<'a> = std::vec::IntoIter<(StorageKey, StorageItem)>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.find(key_prefix, direction)
    }
}

#[cfg(test)]
#[path = "../../tests/persistence/data_cache/cache.rs"]
mod tests;

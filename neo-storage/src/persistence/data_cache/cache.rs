use super::observer::{DataCacheReadObservation, DataCacheReadOrigin};
use super::storage_watch::log_watched_storage_event;
use super::trackable::{DataCacheConfig, DataCacheError, DataCacheResult, InnerState, Trackable};
use crate::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::store::{RawOverlaySink, RawOverlaySource};
use crate::types::{StorageItem, StorageKey, TrackState};
use parking_lot::RwLock;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Weak};
use tracing::warn;

#[path = "observed_reads.rs"]
mod observed_reads;

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
    DetachedParent(Arc<DataCache<B>>),
}

impl<B: CacheRead> CacheBacking<B> {
    fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self {
            Self::Source(source) => source.get(key),
            Self::Parent(parent) | Self::DetachedParent(parent) => parent.get_unobserved(key),
        }
    }

    fn get_with_origin(&self, key: &StorageKey) -> (Option<StorageItem>, DataCacheReadOrigin) {
        match self {
            Self::Source(source) => (source.get(key), DataCacheReadOrigin::PinnedPrefix),
            Self::Parent(parent) => parent.get_unobserved_with_origin(key),
            Self::DetachedParent(parent) => (
                parent.get_unobserved(key),
                DataCacheReadOrigin::PinnedPrefix,
            ),
        }
    }

    fn cached_read_origin(&self, key: &StorageKey) -> DataCacheReadOrigin {
        match self {
            Self::Source(_) | Self::DetachedParent(_) => DataCacheReadOrigin::PinnedPrefix,
            Self::Parent(parent) => parent.cached_read_origin(key),
        }
    }

    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>> {
        match self {
            Self::Source(source) => source.find(prefix, direction),
            Self::Parent(parent) | Self::DetachedParent(parent) => {
                Some(parent.find_unobserved(prefix, direction).collect())
            }
        }
    }

    fn parent(&self) -> Option<&DataCache<B>> {
        match self {
            Self::Parent(parent) => Some(parent.as_ref()),
            Self::Source(_) | Self::DetachedParent(_) => None,
        }
    }

    fn is_detached(&self) -> bool {
        matches!(self, Self::DetachedParent(_))
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
    /// Optional observation shared by this cache and descendant overlays.
    read_observation: Option<Arc<DataCacheReadObservation>>,
    /// Whether local writes belong to a detached transaction overlay.
    inside_detached_overlay: bool,
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

/// Opaque identity and revision of one [`DataCache`] state.
///
/// Tokens are process-local and bound to the exact shared cache state that
/// produced them. A token from a child overlay, isolated fork, or unrelated
/// cache is rejected even when its numeric revision happens to match.
#[derive(Clone)]
pub struct DataCacheVersion {
    state: Weak<RwLock<InnerState>>,
    revision: u64,
}

impl DataCacheVersion {
    /// Monotonic revision within this cache state.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    fn belongs_to(&self, state: &Arc<RwLock<InnerState>>) -> bool {
        Weak::ptr_eq(&self.state, &Arc::downgrade(state))
    }
}

impl fmt::Debug for DataCacheVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DataCacheVersion")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}

impl PartialEq for DataCacheVersion {
    fn eq(&self, other: &Self) -> bool {
        self.revision == other.revision && Weak::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for DataCacheVersion {}

/// Read-only point lookup over a cache while its state is exclusively locked.
///
/// The view never tracks reads or invokes observers. It exists only for the
/// validation callback passed to
/// [`DataCache::try_validate_and_merge_tracked_items`].
pub struct LockedDataCacheView<'a, B = EmptyCacheBacking> {
    state: &'a InnerState,
    backing: &'a CacheBacking<B>,
}

impl<B: CacheRead> LockedDataCacheView<'_, B> {
    /// Returns the value visible in the exclusively locked cache state.
    #[must_use]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        match self.state.dictionary.get(key) {
            Some(trackable) => visible_trackable_item(trackable),
            None => self.backing.get(key),
        }
    }
}

/// Failure from an atomic cache validation and effect publication attempt.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DataCacheAtomicMergeError<E> {
    /// The admission token belongs to another cache state.
    #[error("cache version token belongs to another cache state")]
    ForeignVersion,
    /// This cache changed after the caller captured its admission token.
    #[error("cache revision changed from {expected} to {actual}")]
    StaleVersion {
        /// Revision carried by the admission token.
        expected: u64,
        /// Revision held under the exclusive publication lock.
        actual: u64,
    },
    /// The caller's dependency validator rejected the current locked state.
    #[error("atomic cache validation rejected publication")]
    Validation(E),
    /// The effect batch could not be merged into the current cache state.
    #[error(transparent)]
    Merge(#[from] DataCacheError),
}

enum PreparedMerge {
    Upsert(StorageKey, Trackable),
    Remove(StorageKey),
    Noop,
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

fn collect_change_overlays(
    state: &InnerState,
    key_prefix: Option<&StorageKey>,
    prefix_bytes: Option<&[u8]>,
) -> BTreeMap<StorageKey, Option<StorageItem>> {
    let mut overlays = BTreeMap::new();
    let mut collect = |key: &StorageKey| {
        let Some(trackable) = state.dictionary.get(key) else {
            return;
        };
        if let Some(item) = overlay_item(trackable) {
            overlays.insert(key.clone(), item);
        }
    };

    if let Some(key_prefix) = key_prefix {
        for key in state.change_set.range(key_prefix.clone()..) {
            if !key_matches_prefix(key, prefix_bytes) {
                break;
            }
            collect(key);
        }
    } else {
        for key in &state.change_set {
            collect(key);
        }
    }

    overlays
}

impl<B: CacheRead> Clone for DataCache<B> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            read_only: self.read_only,
            backing: self.backing.clone(),
            ref_count: Arc::clone(&self.ref_count),
            config: self.config,
            read_observation: self.read_observation.as_ref().map(Arc::clone),
            inside_detached_overlay: self.inside_detached_overlay,
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
            read_observation: None,
            inside_detached_overlay: false,
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
        if self.backing.is_detached() {
            return Err(DataCacheError::DetachedCommit);
        }
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

    /// Captures an admission token for a later atomic validation and merge.
    ///
    /// Any cache-state mutation, including read-cache population, invalidates
    /// the token. The token is also bound to this exact shared cache state.
    #[must_use]
    pub fn version(&self) -> DataCacheVersion {
        let state = self.state.read();
        DataCacheVersion {
            state: Arc::downgrade(&self.state),
            revision: state.revision,
        }
    }

    /// Validates dependencies and publishes a tracked-effect batch atomically.
    ///
    /// The method acquires one exclusive cache-state lock, verifies
    /// `expected_version`, preflights every effect, invokes `validate` through
    /// an unobserved locked view, and only then publishes all effects. A stale
    /// token, validation rejection, or invalid effect leaves the destination
    /// unchanged. Duplicate effect keys are rejected to keep preflight and
    /// publication semantics unambiguous.
    ///
    /// The backing [`CacheRead`] must provide the same point-in-time semantics
    /// it provides to ordinary `DataCache` reads. Canonical block execution
    /// satisfies this by keeping durable-store publication outside the cache
    /// application lane.
    ///
    /// `validate` must use the supplied [`LockedDataCacheView`] for cache
    /// lookups. Re-entering this `DataCache` from the callback would attempt to
    /// acquire its already-held write lock.
    pub fn try_validate_and_merge_tracked_items<T, E>(
        &self,
        expected_version: &DataCacheVersion,
        items: &[(StorageKey, Trackable)],
        validate: impl FnOnce(&LockedDataCacheView<'_, B>) -> Result<T, E>,
    ) -> Result<T, DataCacheAtomicMergeError<E>> {
        if !expected_version.belongs_to(&self.state) {
            return Err(DataCacheAtomicMergeError::ForeignVersion);
        }

        let mut state = self.state.write();
        if expected_version.revision != state.revision {
            return Err(DataCacheAtomicMergeError::StaleVersion {
                expected: expected_version.revision,
                actual: state.revision,
            });
        }
        if !items.is_empty() {
            self.ensure_writable()
                .map_err(DataCacheAtomicMergeError::Merge)?;
        }

        let prepared = self
            .prepare_merge_locked(&state, items)
            .map_err(DataCacheAtomicMergeError::Merge)?;
        let validation = validate(&LockedDataCacheView {
            state: &state,
            backing: &self.backing,
        })
        .map_err(DataCacheAtomicMergeError::Validation)?;

        let mut modified = false;
        for effect in prepared {
            match effect {
                PreparedMerge::Upsert(key, trackable) => {
                    log_watched_storage_event(
                        "merge",
                        "atomic_merge_upsert",
                        &key,
                        state.dictionary.get(&key).map(|entry| entry.state),
                        Some(trackable.state),
                        Some(&trackable.item),
                    );
                    state.change_set.insert(key.clone());
                    state.dictionary.insert(key, trackable);
                    modified = true;
                }
                PreparedMerge::Remove(key) => {
                    log_watched_storage_event(
                        "merge",
                        "atomic_merge_remove",
                        &key,
                        state.dictionary.get(&key).map(|entry| entry.state),
                        Some(TrackState::NotFound),
                        None,
                    );
                    state.dictionary.remove(&key);
                    state.change_set.remove(&key);
                    modified = true;
                }
                PreparedMerge::Noop => {}
            }
        }
        if modified {
            state.bump_revision();
        }

        Ok(validation)
    }

    fn prepare_merge_locked(
        &self,
        state: &InnerState,
        items: &[(StorageKey, Trackable)],
    ) -> DataCacheResult<Vec<PreparedMerge>> {
        let mut seen = BTreeSet::new();
        let mut prepared = Vec::with_capacity(items.len());
        for (key, incoming) in items {
            if !seen.insert(key.clone()) {
                return Err(DataCacheError::DuplicateMergeKey(key.clone()));
            }

            let current = state
                .dictionary
                .get(key)
                .map(|entry| entry.state)
                .unwrap_or(TrackState::NotFound);
            let effect = match incoming.state {
                TrackState::Added => match current {
                    TrackState::Deleted => PreparedMerge::Upsert(
                        key.clone(),
                        Trackable::new(incoming.item.clone(), TrackState::Changed),
                    ),
                    TrackState::NotFound => PreparedMerge::Upsert(
                        key.clone(),
                        Trackable::new(incoming.item.clone(), TrackState::Added),
                    ),
                    TrackState::Added | TrackState::Changed | TrackState::None => {
                        return Err(DataCacheError::InvalidMergeState {
                            key: key.clone(),
                            incoming: incoming.state,
                            current,
                        });
                    }
                },
                TrackState::Changed => {
                    let merged = if current == TrackState::Added {
                        TrackState::Added
                    } else {
                        TrackState::Changed
                    };
                    PreparedMerge::Upsert(
                        key.clone(),
                        Trackable::new(incoming.item.clone(), merged),
                    )
                }
                TrackState::Deleted => match current {
                    TrackState::Added => PreparedMerge::Remove(key.clone()),
                    TrackState::Changed | TrackState::None => PreparedMerge::Upsert(
                        key.clone(),
                        Trackable::new(StorageItem::default(), TrackState::Deleted),
                    ),
                    TrackState::NotFound if self.backing.get(key).is_some() => {
                        PreparedMerge::Upsert(
                            key.clone(),
                            Trackable::new(StorageItem::default(), TrackState::Deleted),
                        )
                    }
                    TrackState::NotFound | TrackState::Deleted => PreparedMerge::Noop,
                },
                TrackState::None | TrackState::NotFound => {
                    return Err(DataCacheError::InvalidMergeState {
                        key: key.clone(),
                        incoming: incoming.state,
                        current,
                    });
                }
            };
            prepared.push(effect);
        }
        Ok(prepared)
    }

    /// Creates a cloned overlay cache that uses this cache as the backing store.
    pub fn clone_cache(&self) -> Self {
        self.clone_cache_with_config(self.config)
    }

    /// Creates a cloned overlay with an explicit [`DataCacheConfig`].
    ///
    /// Transaction and nested execution snapshots typically set
    /// `track_reads_in_write_cache = false`: first-time gets stay parent-backed
    /// without a child write-lock + clone, while puts/deletes still populate
    /// the change set. That matches committed value semantics and is faster on
    /// read-heavy contract paths where most keys are touched once.
    pub fn clone_cache_with_config(&self, config: DataCacheConfig) -> Self {
        self.clone_overlay_with_config(config, false)
    }

    /// Creates a writable transaction overlay that can never publish into this cache.
    ///
    /// Reads resolve through this cache and nested ordinary child caches may
    /// commit into the detached overlay. Committing the detached root itself is
    /// rejected, so speculative effects remain invisible until a validator
    /// applies them through a separate canonical path.
    pub fn clone_detached_cache(&self) -> Self {
        self.clone_detached_cache_with_config(self.config)
    }

    /// Creates a detached transaction overlay with an explicit configuration.
    pub fn clone_detached_cache_with_config(&self, config: DataCacheConfig) -> Self {
        self.clone_overlay_with_config(config, true)
    }

    fn clone_overlay_with_config(&self, config: DataCacheConfig, detached: bool) -> Self {
        let parent = Arc::new(self.clone());
        Self {
            state: Arc::new(RwLock::new(InnerState::new())),
            read_only: false,
            backing: if detached {
                CacheBacking::DetachedParent(parent)
            } else {
                CacheBacking::Parent(parent)
            },
            ref_count: Arc::new(AtomicUsize::new(1)),
            config,
            read_observation: self.read_observation.as_ref().map(Arc::clone),
            inside_detached_overlay: detached || self.inside_detached_overlay,
            #[cfg(test)]
            merge_write_passes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_items_calls: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn merge_tracked_items_from(&self, source: &DataCache<B>) {
        if let Some(observation) = source
            .active_read_observation()
            .or_else(|| self.active_read_observation())
        {
            self.merge_tracked_items_observed(source, &observation);
        } else {
            self.merge_tracked_items_without_update_callbacks(source);
        }
    }

    fn merge_tracked_items_without_update_callbacks(&self, source: &DataCache<B>) {
        let source_state = source.state.read();
        if source_state.change_set.is_empty() {
            return;
        }

        let mut state = self.state.write();
        let mut modified = false;
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
                    modified = true;
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
                    modified = true;
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
                        modified = true;
                    }
                    TrackState::Changed | TrackState::None => {
                        state.dictionary.insert(
                            key.clone(),
                            Trackable::new(StorageItem::default(), TrackState::Deleted),
                        );
                        state.change_set.insert(key.clone());
                        modified = true;
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
                        let store_item = self.backing.get(key);
                        let exists_in_store = store_item.is_some();
                        if exists_in_store {
                            state.dictionary.insert(
                                key.clone(),
                                Trackable::new(StorageItem::default(), TrackState::Deleted),
                            );
                            state.change_set.insert(key.clone());
                            modified = true;
                        }
                    }
                    TrackState::Deleted => {}
                },
                TrackState::None | TrackState::NotFound => {}
            }
        }
        if modified {
            state.bump_revision();
        }
    }

    /// Merges tracked changes from another cache into this one.
    pub fn merge_tracked_items(&self, items: &[(StorageKey, Trackable)]) {
        for (key, trackable) in items {
            self.merge_tracked_item(key, trackable);
        }
    }

    fn merge_tracked_item(&self, key: &StorageKey, trackable: &Trackable) {
        self.merge_tracked_item_with_observation(key, trackable, self.read_observation.as_deref());
    }

    fn merge_tracked_item_with_observation(
        &self,
        key: &StorageKey,
        trackable: &Trackable,
        observation: Option<&DataCacheReadObservation>,
    ) {
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
            TrackState::Deleted => self.apply_delete_with_observation(key, observation),
            TrackState::None | TrackState::NotFound => {}
        }
    }

    /// Creates an isolated writable fork backed by the same underlying store.
    pub fn fork_isolated(&self) -> Self {
        let state = self.state.read();
        let cloned_state = InnerState {
            dictionary: state.dictionary.clone(),
            change_set: state.change_set.clone(),
            revision: state.revision,
        };
        drop(state);

        Self {
            state: Arc::new(RwLock::new(cloned_state)),
            read_only: self.read_only,
            backing: self.backing.clone(),
            ref_count: Arc::new(AtomicUsize::new(1)),
            config: self.config,
            read_observation: self.read_observation.as_ref().map(Arc::clone),
            inside_detached_overlay: self.inside_detached_overlay,
            #[cfg(test)]
            merge_write_passes: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_items_calls: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            tracked_item_visit_calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn track_in_write_cache(&self, key: &StorageKey, item: &StorageItem) {
        if !self.config.track_reads_in_write_cache {
            return;
        }
        let mut state = self.state.write();
        if state.dictionary.len() < self.config.max_entries {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                state.dictionary.entry(key.clone())
            {
                entry.insert(Trackable::new(item.clone(), TrackState::None));
                state.bump_revision();
            }
        }
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
        state.bump_revision();
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
        state.bump_revision();
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
        self.apply_delete_with_observation(key, self.read_observation.as_deref());
    }

    fn apply_delete_with_observation(
        &self,
        key: &StorageKey,
        observation: Option<&DataCacheReadObservation>,
    ) {
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
                state.bump_revision();
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
                state.bump_revision();
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
                let (store_item, origin) = observation.map_or_else(
                    || (self.backing.get(key), DataCacheReadOrigin::PinnedPrefix),
                    |_| self.backing.get_with_origin(key),
                );
                if let Some(observation) = observation {
                    observation.observe_point(key, store_item.as_ref(), origin);
                }
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
                        state.bump_revision();
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
                        state.bump_revision();
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
        if self.backing.is_detached() {
            warn!("attempted to commit a detached DataCache root");
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
        let keys = std::mem::take(&mut state.change_set);
        let modified = !keys.is_empty();
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
        if modified {
            state.bump_revision();
        }
    }

    /// Resets the cache for reuse.
    pub fn reset(&self) {
        let mut state = self.state.write();
        let modified = !state.dictionary.is_empty() || !state.change_set.is_empty();
        state.dictionary.clear();
        state.change_set.clear();
        if modified {
            state.bump_revision();
        }
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
    /// for storage engines that benefit from ordered, page-local writes.
    pub fn visit_tracked_items_sorted<F>(&self, mut visit: F)
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

#[cfg(test)]
#[path = "../../tests/persistence/data_cache/observer.rs"]
mod observer_tests;

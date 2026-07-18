use super::{
    CacheRead, DataCache, DataCacheReadObservation, collect_change_overlays, key_matches_prefix,
    log_watched_storage_event, visible_trackable_item,
};
use crate::persistence::data_cache::DataCacheReadOrigin;
use crate::persistence::data_cache::observer::pause;
use crate::{
    DataCacheReadObservationPause, DataCacheReadObserver, SeekDirection, StorageItem, StorageKey,
    TrackState,
};
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use std::sync::Arc;

impl<B: CacheRead> DataCache<B> {
    /// Installs an observer inherited by every subsequently cloned overlay.
    #[must_use]
    pub fn with_read_observer(mut self, observer: Arc<dyn DataCacheReadObserver>) -> Self {
        self.read_observation = Some(Arc::new(DataCacheReadObservation::new(observer)));
        self
    }

    /// Returns whether this cache carries an observer, including while paused.
    #[must_use]
    #[inline]
    pub fn has_read_observer(&self) -> bool {
        self.read_observation.is_some()
    }

    pub(super) fn active_read_observation(&self) -> Option<Arc<DataCacheReadObservation>> {
        self.read_observation
            .as_ref()
            .filter(|observation| observation.is_active())
            .map(Arc::clone)
    }

    /// Pauses this observer and the same observer inherited by child overlays.
    #[must_use]
    pub fn pause_read_observation(&self) -> DataCacheReadObservationPause {
        pause(self.read_observation.as_ref())
    }

    /// Permanently disables this observer without changing cache behavior.
    pub fn disable_read_observation(&self) {
        if let Some(observation) = &self.read_observation {
            observation.disable();
        }
    }

    pub(super) fn merge_tracked_items_observed(
        &self,
        source: &DataCache<B>,
        observation: &DataCacheReadObservation,
    ) {
        let mut previous = None;
        loop {
            let next = {
                let state = source.state.read();
                let key = match previous.as_ref() {
                    Some(previous) => state
                        .change_set
                        .range((Excluded(previous), Unbounded))
                        .next(),
                    None => state.change_set.first(),
                };
                key.map(|key| (key.clone(), state.dictionary.get(key).cloned()))
            };
            let Some((key, trackable)) = next else {
                break;
            };
            previous = Some(key.clone());
            if let Some(trackable) = trackable {
                self.merge_tracked_item_with_observation(&key, &trackable, Some(observation));
            }
        }
    }

    /// Gets an item from the cache.
    #[inline]
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get_inner(key, true)
    }

    #[inline]
    pub(super) fn get_unobserved(&self, key: &StorageKey) -> Option<StorageItem> {
        self.get_inner(key, false)
    }

    pub(super) fn get_unobserved_with_origin(
        &self,
        key: &StorageKey,
    ) -> (Option<StorageItem>, DataCacheReadOrigin) {
        self.get_inner_with_origin(key, false)
    }

    fn get_inner(&self, key: &StorageKey, observe: bool) -> Option<StorageItem> {
        if observe && self.read_observation.is_some() {
            self.get_inner_with_origin(key, true).0
        } else {
            self.get_inner_unclassified(key)
        }
    }

    fn get_inner_unclassified(&self, key: &StorageKey) -> Option<StorageItem> {
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

        if let Some(item) = self.backing.get(key) {
            self.track_in_write_cache(key, &item);
            log_watched_storage_event("get", "store_get_hit", key, None, None, Some(&item));
            return Some(item);
        }

        log_watched_storage_event("get", "miss", key, None, None, None);
        None
    }

    fn get_inner_with_origin(
        &self,
        key: &StorageKey,
        observe: bool,
    ) -> (Option<StorageItem>, DataCacheReadOrigin) {
        {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                let track_state = trackable.state;
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
                    let item = trackable.item.clone();
                    drop(state);
                    let origin = self.point_origin(key, track_state);
                    self.observe_point_if_enabled(observe, key, Some(&item), origin);
                    return (Some(item), origin);
                }
                log_watched_storage_event(
                    "get",
                    "dictionary_deleted",
                    key,
                    Some(trackable.state),
                    Some(trackable.state),
                    None,
                );
                drop(state);
                let origin = self.point_origin(key, track_state);
                self.observe_point_if_enabled(observe, key, None, origin);
                return (None, origin);
            }
        }

        let (backing_item, origin) = self.backing.get_with_origin(key);
        if let Some(item) = backing_item {
            self.track_in_write_cache(key, &item);
            log_watched_storage_event("get", "store_get_hit", key, None, None, Some(&item));
            self.observe_point_if_enabled(observe, key, Some(&item), origin);
            return (Some(item), origin);
        }

        log_watched_storage_event("get", "miss", key, None, None, None);
        self.observe_point_if_enabled(observe, key, None, origin);
        (None, origin)
    }

    fn point_origin(&self, key: &StorageKey, state: TrackState) -> DataCacheReadOrigin {
        if self.inside_detached_overlay
            && matches!(
                state,
                TrackState::Added | TrackState::Changed | TrackState::Deleted
            )
        {
            DataCacheReadOrigin::Overlay
        } else if matches!(state, TrackState::None | TrackState::NotFound) {
            self.backing.cached_read_origin(key)
        } else {
            DataCacheReadOrigin::PinnedPrefix
        }
    }

    pub(super) fn cached_read_origin(&self, key: &StorageKey) -> DataCacheReadOrigin {
        let state = self
            .state
            .read()
            .dictionary
            .get(key)
            .map(|trackable| trackable.state);
        state.map_or_else(
            || self.backing.cached_read_origin(key),
            |state| self.point_origin(key, state),
        )
    }

    #[inline]
    pub(super) fn observe_point_if_enabled(
        &self,
        observe: bool,
        key: &StorageKey,
        value: Option<&StorageItem>,
        origin: DataCacheReadOrigin,
    ) {
        if observe && let Some(observation) = &self.read_observation {
            observation.observe_point(key, value, origin);
        }
    }

    /// Gets an item from the cache as a reference.
    #[inline]
    pub fn get_ref(&self, key: &StorageKey) -> Option<StorageItem> {
        let (value, state) = {
            let state = self.state.read();
            if let Some(trackable) = state.dictionary.get(key) {
                if trackable.state != TrackState::Deleted && trackable.state != TrackState::NotFound
                {
                    (Some(trackable.item.clone()), Some(trackable.state))
                } else {
                    (None, Some(trackable.state))
                }
            } else {
                (None, None)
            }
        };
        if self.read_observation.is_none() {
            return value;
        }
        let origin = state.map_or(DataCacheReadOrigin::PinnedPrefix, |state| {
            self.point_origin(key, state)
        });
        self.observe_point_if_enabled(true, key, value.as_ref(), origin);
        value
    }

    /// Finds items by key prefix.
    pub fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> std::vec::IntoIter<(StorageKey, StorageItem)> {
        self.find_inner(key_prefix, direction, true)
    }

    pub(super) fn find_unobserved(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> std::vec::IntoIter<(StorageKey, StorageItem)> {
        self.find_inner(key_prefix, direction, false)
    }

    fn find_inner(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
        observe: bool,
    ) -> std::vec::IntoIter<(StorageKey, StorageItem)> {
        let prefix_bytes = key_prefix.map(|key| key.as_bytes().into_owned());
        let entries = if let Some(backing_entries) = self.backing.find(key_prefix, direction) {
            let state = self.state.read();
            let overlays = collect_change_overlays(&state, key_prefix, prefix_bytes.as_deref());
            drop(state);

            if overlays.is_empty() {
                let prefix_bytes = prefix_bytes.clone();
                backing_entries
                    .into_iter()
                    .filter(move |(key, _)| key_matches_prefix(key, prefix_bytes.as_deref()))
                    .collect::<Vec<_>>()
            } else {
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
                entries
            }
        } else {
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
            entries
        };
        if observe && let Some(observation) = &self.read_observation {
            observation.observe_range(key_prefix, direction, &entries);
        }
        entries.into_iter()
    }
}

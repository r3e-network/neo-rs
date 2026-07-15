use crate::types::{StorageItem, StorageKey, TrackState};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;
use std::fmt;
use thiserror::Error;

/// Represents an entry in the cache with tracking state.
///
/// Wraps a stored value with its [`TrackState`] so the cache can compute the
/// minimal set of changes to flush on commit.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct TrackableEntry<T> {
    /// The storage item data.
    pub item: T,
    /// The tracking state of this entry.
    pub state: TrackState,
}

/// Storage-item trackable entry used by [`DataCache`](super::cache::DataCache).
pub type Trackable = TrackableEntry<StorageItem>;

impl<T> TrackableEntry<T> {
    /// Creates a new trackable entry.
    #[must_use]
    pub const fn new(item: T, state: TrackState) -> Self {
        Self { item, state }
    }

    /// Creates a trackable entry with `TrackState::None`.
    #[must_use]
    pub fn unchanged(item: T) -> Self {
        Self::new(item, TrackState::None)
    }

    /// Creates a trackable entry with `TrackState::Added`.
    #[must_use]
    pub fn added(item: T) -> Self {
        Self::new(item, TrackState::Added)
    }

    /// Creates a trackable entry with `TrackState::Changed`.
    #[must_use]
    pub fn changed(item: T) -> Self {
        Self::new(item, TrackState::Changed)
    }

    /// Returns whether this entry has been modified (added, changed, or deleted).
    #[must_use]
    pub const fn is_modified(&self) -> bool {
        matches!(
            self.state,
            TrackState::Added | TrackState::Changed | TrackState::Deleted
        )
    }

    /// Returns whether this entry should be persisted on commit.
    #[must_use]
    pub const fn should_persist(&self) -> bool {
        matches!(self.state, TrackState::Added | TrackState::Changed)
    }

    /// Returns whether this entry should be removed on commit.
    #[must_use]
    pub const fn should_delete(&self) -> bool {
        matches!(self.state, TrackState::Deleted)
    }
}

impl<T: Default> TrackableEntry<T> {
    /// Creates a trackable entry with `TrackState::Deleted`.
    #[must_use]
    pub fn deleted() -> Self {
        Self::new(T::default(), TrackState::Deleted)
    }
}

impl<T: fmt::Debug> fmt::Debug for TrackableEntry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trackable")
            .field("item", &self.item)
            .field("state", &self.state)
            .finish()
    }
}

/// Internal state protected by RwLock for thread-safe Copy-on-Write
pub(crate) struct InnerState {
    /// Hot lookup table for storage keys during TX/native execution.
    ///
    /// `FxHashMap` is faster than the std hasher for short contract-storage
    /// keys on the import path (every `System.Storage.Get`/`Put` hits this map).
    pub(crate) dictionary: FxHashMap<StorageKey, Trackable>,
    pub(crate) change_set: BTreeSet<StorageKey>,
}

impl InnerState {
    pub(crate) fn new() -> Self {
        Self {
            dictionary: FxHashMap::default(),
            change_set: BTreeSet::new(),
        }
    }
}

/// Configuration for DataCache.
#[derive(Debug, Clone, Copy)]
pub struct DataCacheConfig {
    /// Maximum number of entries in the write cache
    pub max_entries: usize,
    /// Whether reads should be mirrored into the write cache dictionary.
    pub track_reads_in_write_cache: bool,
}

impl Default for DataCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 100000,
            track_reads_in_write_cache: true,
        }
    }
}

/// Errors returned by DataCache operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataCacheError {
    /// The cache is read-only and cannot accept mutations.
    #[error("cache is read-only")]
    ReadOnly,
    /// The requested mutation is not valid for the entry's current tracked state.
    #[error("element currently has state {0:?}")]
    InvalidState(TrackState),
    /// Persisting the tracked change set into the backing store failed.
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),
}

/// Result type for [`DataCache`](super::cache::DataCache) mutation operations.
pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;

#[cfg(test)]
#[path = "../../tests/persistence/data_cache/trackable.rs"]
mod tests;

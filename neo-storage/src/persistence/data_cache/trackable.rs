use crate::persistence::read_cache::ReadCacheConfig;
use crate::types::{StorageItem, StorageKey, TrackState};
use std::collections::{HashMap, HashSet};
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
    pub(crate) dictionary: HashMap<StorageKey, Trackable>,
    pub(crate) change_set: HashSet<StorageKey>,
}

impl InnerState {
    pub(crate) fn new() -> Self {
        Self {
            dictionary: HashMap::new(),
            change_set: HashSet::new(),
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

/// Errors returned by DataCache operations.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DataCacheError {
    #[error("cache is read-only")]
    ReadOnly,
    #[error("unable to commit changes: {0}")]
    CommitFailed(String),
}

pub type DataCacheResult<T = ()> = Result<T, DataCacheError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trackable_new() {
        let item = StorageItem::from_bytes(vec![0xAA]);
        let trackable = Trackable::new(item.clone(), TrackState::Added);
        assert_eq!(trackable.item, item);
        assert_eq!(trackable.state, TrackState::Added);
    }

    #[test]
    fn test_trackable_state_helpers() {
        assert!(!Trackable::unchanged(StorageItem::default()).is_modified());
        assert!(Trackable::added(StorageItem::default()).is_modified());
        assert!(Trackable::changed(StorageItem::default()).is_modified());
        assert!(Trackable::deleted().is_modified());

        assert!(Trackable::added(StorageItem::default()).should_persist());
        assert!(!Trackable::deleted().should_persist());
        assert!(Trackable::deleted().should_delete());
    }

    #[test]
    fn test_trackable_default_and_clone() {
        let trackable = Trackable::default();
        assert_eq!(trackable.state, TrackState::None);

        let original = Trackable::added(StorageItem::from_bytes(vec![0x01, 0x02]));
        assert_eq!(original, original.clone());
    }

    #[test]
    fn test_trackable_debug_and_not_found() {
        let trackable = Trackable::added(StorageItem::from_bytes(vec![0x01]));
        let debug = format!("{:?}", trackable);
        assert!(debug.contains("Trackable"));
        assert!(debug.contains("Added"));

        let nf = Trackable::new(StorageItem::default(), TrackState::NotFound);
        assert!(!nf.is_modified());
        assert!(!nf.should_persist());
        assert!(!nf.should_delete());
    }
}

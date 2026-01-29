//! Trackable entry for cache state management.
//!
//! Wraps storage items with tracking state for change detection.

use crate::types::{StorageItem, TrackState};

/// Represents an entry in the cache with tracking state.
///
/// Used by [`DataCache`](super::DataCache) to track the state of each entry
/// for efficient commit operations.
#[derive(Debug, Clone)]
pub struct Trackable {
    /// The storage item data.
    pub item: StorageItem,
    /// The tracking state of this entry.
    pub state: TrackState,
}

impl Trackable {
    /// Creates a new trackable entry.
    #[must_use] 
    pub const fn new(item: StorageItem, state: TrackState) -> Self {
        Self { item, state }
    }

    /// Creates a trackable entry with `TrackState::None`.
    #[must_use] 
    pub fn unchanged(item: StorageItem) -> Self {
        Self::new(item, TrackState::None)
    }

    /// Creates a trackable entry with `TrackState::Added`.
    #[must_use] 
    pub fn added(item: StorageItem) -> Self {
        Self::new(item, TrackState::Added)
    }

    /// Creates a trackable entry with `TrackState::Changed`.
    #[must_use] 
    pub fn changed(item: StorageItem) -> Self {
        Self::new(item, TrackState::Changed)
    }

    /// Creates a trackable entry with `TrackState::Deleted`.
    #[must_use] 
    pub fn deleted() -> Self {
        Self::new(StorageItem::default(), TrackState::Deleted)
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

impl Default for Trackable {
    fn default() -> Self {
        Self::unchanged(StorageItem::default())
    }
}

impl PartialEq for Trackable {
    fn eq(&self, other: &Self) -> bool {
        self.item == other.item && self.state == other.state
    }
}

impl Eq for Trackable {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trackable_new() {
        let item = StorageItem::new(vec![0xAA]);
        let trackable = Trackable::new(item.clone(), TrackState::Added);
        assert_eq!(trackable.item, item);
        assert_eq!(trackable.state, TrackState::Added);
    }

    #[test]
    fn test_trackable_unchanged() {
        let item = StorageItem::new(vec![0x01]);
        let trackable = Trackable::unchanged(item.clone());
        assert_eq!(trackable.state, TrackState::None);
        assert!(!trackable.is_modified());
    }

    #[test]
    fn test_trackable_added() {
        let item = StorageItem::new(vec![0x02]);
        let trackable = Trackable::added(item);
        assert_eq!(trackable.state, TrackState::Added);
        assert!(trackable.is_modified());
        assert!(trackable.should_persist());
    }

    #[test]
    fn test_trackable_changed() {
        let item = StorageItem::new(vec![0x03]);
        let trackable = Trackable::changed(item);
        assert_eq!(trackable.state, TrackState::Changed);
        assert!(trackable.is_modified());
        assert!(trackable.should_persist());
    }

    #[test]
    fn test_trackable_deleted() {
        let trackable = Trackable::deleted();
        assert_eq!(trackable.state, TrackState::Deleted);
        assert!(trackable.is_modified());
        assert!(trackable.should_delete());
        assert!(!trackable.should_persist());
    }

    #[test]
    fn test_trackable_is_modified() {
        assert!(!Trackable::unchanged(StorageItem::default()).is_modified());
        assert!(Trackable::added(StorageItem::default()).is_modified());
        assert!(Trackable::changed(StorageItem::default()).is_modified());
        assert!(Trackable::deleted().is_modified());
    }

    #[test]
    fn test_trackable_should_persist() {
        assert!(!Trackable::unchanged(StorageItem::default()).should_persist());
        assert!(Trackable::added(StorageItem::default()).should_persist());
        assert!(Trackable::changed(StorageItem::default()).should_persist());
        assert!(!Trackable::deleted().should_persist());
    }

    #[test]
    fn test_trackable_should_delete() {
        assert!(!Trackable::unchanged(StorageItem::default()).should_delete());
        assert!(!Trackable::added(StorageItem::default()).should_delete());
        assert!(!Trackable::changed(StorageItem::default()).should_delete());
        assert!(Trackable::deleted().should_delete());
    }

    #[test]
    fn test_trackable_default() {
        let trackable = Trackable::default();
        assert_eq!(trackable.state, TrackState::None);
    }

    #[test]
    fn test_trackable_clone() {
        let original = Trackable::added(StorageItem::new(vec![0x01, 0x02]));
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_trackable_equality() {
        let t1 = Trackable::added(StorageItem::new(vec![0x01]));
        let t2 = Trackable::added(StorageItem::new(vec![0x01]));
        let t3 = Trackable::changed(StorageItem::new(vec![0x01]));
        let t4 = Trackable::added(StorageItem::new(vec![0x02]));

        assert_eq!(t1, t2);
        assert_ne!(t1, t3); // Different state
        assert_ne!(t1, t4); // Different item
    }

    #[test]
    fn test_trackable_debug() {
        let trackable = Trackable::added(StorageItem::new(vec![0x01]));
        let debug = format!("{:?}", trackable);
        assert!(debug.contains("Trackable"));
        assert!(debug.contains("Added"));
    }

    #[test]
    fn test_trackable_not_found_state() {
        let trackable = Trackable::new(StorageItem::default(), TrackState::NotFound);
        assert!(!trackable.is_modified());
        assert!(!trackable.should_persist());
        assert!(!trackable.should_delete());
    }
}

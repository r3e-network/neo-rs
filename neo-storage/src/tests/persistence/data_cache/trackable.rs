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

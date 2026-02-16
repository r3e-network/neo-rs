use super::*;
use crate::smart_contract::StorageKey;

fn make_key(id: i32, suffix: &[u8]) -> StorageKey {
    StorageKey::new(id, suffix.to_vec())
}

#[test]
fn clone_cache_preserves_entries_and_change_set() {
    let cache = DataCache::new(false);
    let key = make_key(1, b"a");
    cache.add(key.clone(), StorageItem::from_bytes(vec![42]));

    let cloned = cache.clone_cache();

    assert_eq!(
        cloned.get(&key).unwrap().get_value(),
        vec![42],
        "cloned cache should contain original entry"
    );

    let change_set = cloned.get_change_set();
    assert!(
        change_set.contains(&key),
        "clone should retain pending change set entries"
    );
}

#[test]
fn merge_tracked_items_applies_changes() {
    let base = DataCache::new(false);
    let key_added = make_key(2, b"b");
    let key_updated = make_key(3, b"c");

    base.add(key_updated.clone(), StorageItem::from_bytes(vec![1]));

    let clone = base.clone_cache();
    clone.add(key_added.clone(), StorageItem::from_bytes(vec![7]));
    clone.update(key_updated.clone(), StorageItem::from_bytes(vec![9]));

    let tracked = clone.tracked_items();
    base.merge_tracked_items(&tracked);

    assert_eq!(
        base.get(&key_added).unwrap().get_value(),
        vec![7],
        "merge should add new items"
    );
    assert_eq!(
        base.get(&key_updated).unwrap().get_value(),
        vec![9],
        "merge should update existing items"
    );
}

#[test]
fn read_only_cache_rejects_mutations() {
    let cache = DataCache::new(true);
    let key = make_key(9, b"x");
    let item = StorageItem::from_bytes(vec![1]);

    assert_eq!(
        cache.try_add(key.clone(), item.clone()),
        Err(DataCacheError::ReadOnly)
    );
    assert_eq!(
        cache.try_update(key.clone(), item.clone()),
        Err(DataCacheError::ReadOnly)
    );
    assert_eq!(cache.try_delete(&key), Err(DataCacheError::ReadOnly));
    assert_eq!(cache.try_commit(), Err(DataCacheError::ReadOnly));
    assert!(cache.get(&key).is_none());
    assert!(cache.tracked_items().is_empty());
}

#[test]
fn pending_change_count_tracks_changes() {
    let cache = DataCache::new(false);
    assert_eq!(cache.pending_change_count(), 0);
    assert!(!cache.has_pending_changes());

    cache.add(make_key(1, b"a"), StorageItem::from_bytes(vec![1]));
    assert_eq!(cache.pending_change_count(), 1);
    assert!(cache.has_pending_changes());

    cache.add(make_key(2, b"b"), StorageItem::from_bytes(vec![2]));
    assert_eq!(cache.pending_change_count(), 2);
}

#[test]
fn copy_on_write_shares_data() {
    let cache = DataCache::new(false);
    let key = make_key(1, b"test");

    // Add to original cache
    cache.add(key.clone(), StorageItem::from_bytes(vec![1]));

    // Clone should share data
    let cloned = cache.clone_cache();
    assert_eq!(cloned.get(&key).unwrap().get_value(), vec![1]);

    // Modify cloned cache - both should see changes (shared state)
    cloned.add(make_key(2, b"new"), StorageItem::from_bytes(vec![2]));

    // Both should see the new entry (shared state)
    assert_eq!(
        cache.get(&make_key(2, b"new")).unwrap().get_value(),
        vec![2]
    );
}

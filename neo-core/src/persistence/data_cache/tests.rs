use super::*;
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::track_state::TrackState;
use crate::smart_contract::{StorageItem, StorageKey};
use std::sync::Arc;

fn make_key(id: i32, suffix: &[u8]) -> StorageKey {
    StorageKey::new(id, suffix.to_vec())
}

#[test]
fn clone_cache_reads_parent_but_starts_with_empty_change_set() {
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
    assert!(change_set.is_empty(), "cloned overlay should start clean");
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
fn clone_cache_isolated_until_commit() {
    let cache = DataCache::new(false);
    let key = make_key(1, b"test");
    let new_key = make_key(2, b"new");

    // Add to original cache
    cache.add(key.clone(), StorageItem::from_bytes(vec![1]));

    // Clone should be able to read parent data.
    let cloned = cache.clone_cache();
    assert_eq!(cloned.get(&key).unwrap().get_value(), vec![1]);

    // Child writes stay isolated before commit.
    cloned.add(new_key.clone(), StorageItem::from_bytes(vec![2]));
    assert!(cache.get(&new_key).is_none());

    // Commit propagates child changes into the parent cache.
    cloned.commit();
    assert_eq!(cache.get(&new_key).unwrap().get_value(), vec![2]);
}

#[test]
fn delete_uncached_key_without_store_is_noop() {
    let cache = DataCache::new(false);
    let key = make_key(5, b"missing");
    cache.delete(&key);
    assert!(cache.tracked_items().is_empty());
}

#[test]
fn delete_marks_uncached_key_as_deleted_when_backing_store_has_key() {
    let key = make_key(5, b"exists");
    let mut backing_map = hashbrown::HashMap::new();
    backing_map.insert(key.clone(), StorageItem::from_bytes(vec![1]));
    let backing_map = Arc::new(backing_map);

    let getter = {
        let map = Arc::clone(&backing_map);
        Arc::new(move |lookup: &StorageKey| map.get(lookup).cloned())
    };

    let cache = DataCache::new_with_store(false, Some(getter), None);
    cache.delete(&key);

    let tracked = cache.tracked_items();
    assert_eq!(tracked.len(), 1, "delete should produce one tracked change");
    assert_eq!(tracked[0].0, key);
    assert_eq!(tracked[0].1.state, TrackState::Deleted);
}

#[test]
fn find_overlays_changes_and_hides_deleted_store_entries() {
    let key_a = make_key(11, b"a");
    let key_b = make_key(11, b"b");
    let key_c = make_key(11, b"c");

    let mut backing_map = hashbrown::HashMap::new();
    backing_map.insert(key_a.clone(), StorageItem::from_bytes(vec![1]));
    backing_map.insert(key_b.clone(), StorageItem::from_bytes(vec![2]));
    let backing_map = Arc::new(backing_map);

    let getter = {
        let map = Arc::clone(&backing_map);
        Arc::new(move |key: &StorageKey| map.get(key).cloned())
    };
    let finder = {
        let map = Arc::clone(&backing_map);
        Arc::new(
            move |prefix: Option<&StorageKey>,
                  direction: SeekDirection|
                  -> Vec<(StorageKey, StorageItem)> {
                let prefix_bytes = prefix.map(|p| p.to_array());
                let mut items: Vec<_> = map
                    .iter()
                    .filter(|(key, _)| match &prefix_bytes {
                        Some(bytes) => key.to_array().starts_with(bytes),
                        None => true,
                    })
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();

                match direction {
                    SeekDirection::Forward => items.sort_by(|a, b| a.0.cmp(&b.0)),
                    SeekDirection::Backward => items.sort_by(|a, b| b.0.cmp(&a.0)),
                }

                items
            },
        )
    };

    let cache = DataCache::new_with_store(false, Some(getter), Some(finder));
    cache.update(key_a.clone(), StorageItem::from_bytes(vec![9]));
    cache.delete(&key_b);
    cache.add(key_c.clone(), StorageItem::from_bytes(vec![3]));

    let prefix = make_key(11, b"");
    let entries: Vec<_> = cache.find(Some(&prefix), SeekDirection::Forward).collect();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, key_a);
    assert_eq!(entries[0].1.get_value(), vec![9]);
    assert_eq!(entries[1].0, key_c);
    assert_eq!(entries[1].1.get_value(), vec![3]);
}

#[test]
fn find_enforces_prefix_when_backing_iterator_is_range_scan() {
    let key_a = make_key(-1, &[0x08, 0x01]);
    let key_b = make_key(-1, &[0x0c, 0x01]);
    let key_c = make_key(0, &[0x08, 0x01]);

    let mut backing_map = hashbrown::HashMap::new();
    backing_map.insert(key_a.clone(), StorageItem::from_bytes(vec![1]));
    backing_map.insert(key_b.clone(), StorageItem::from_bytes(vec![2]));
    backing_map.insert(key_c.clone(), StorageItem::from_bytes(vec![3]));
    let backing_map = Arc::new(backing_map);

    let getter = {
        let map = Arc::clone(&backing_map);
        Arc::new(move |key: &StorageKey| map.get(key).cloned())
    };
    let finder = {
        let map = Arc::clone(&backing_map);
        Arc::new(
            move |prefix: Option<&StorageKey>,
                  direction: SeekDirection|
                  -> Vec<(StorageKey, StorageItem)> {
                let start = prefix.map(|p| p.to_array());
                let mut items: Vec<_> = map
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();

                match direction {
                    SeekDirection::Forward => {
                        items.sort_by(|a, b| a.0.cmp(&b.0));
                        if let Some(start) = &start {
                            items.retain(|(key, _)| key.to_array() >= *start);
                        }
                    }
                    SeekDirection::Backward => {
                        items.sort_by(|a, b| b.0.cmp(&a.0));
                        if let Some(start) = &start {
                            items.retain(|(key, _)| key.to_array() <= *start);
                        }
                    }
                }

                items
            },
        )
    };

    let cache = DataCache::new_with_store(false, Some(getter), Some(finder));
    let prefix = make_key(-1, &[0x08]);
    let entries: Vec<_> = cache.find(Some(&prefix), SeekDirection::Forward).collect();

    assert_eq!(
        entries.len(),
        1,
        "only matching prefix entries should be returned"
    );
    assert_eq!(entries[0].0, key_a);
    assert_eq!(entries[0].1.get_value(), vec![1]);
}

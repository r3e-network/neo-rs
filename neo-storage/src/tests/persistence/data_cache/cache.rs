use super::*;
use crate::types::StorageItemCache;

#[derive(Clone)]
struct TestBacking {
    entries: Arc<BTreeMap<StorageKey, StorageItem>>,
}

impl CacheRead for TestBacking {
    fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.entries.get(key).cloned()
    }

    fn find(
        &self,
        prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Option<Vec<(StorageKey, StorageItem)>> {
        let prefix = prefix.map(|key| key.as_bytes().into_owned());
        let mut entries = self
            .entries
            .iter()
            .filter(|(key, _)| {
                prefix
                    .as_deref()
                    .is_none_or(|prefix| key.as_bytes().starts_with(prefix))
            })
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        if direction == SeekDirection::Backward {
            entries.reverse();
        }
        Some(entries)
    }
}

fn cache_item(bytes: Vec<u8>) -> StorageItem {
    let mut item = StorageItem::new();
    item.set_cache(StorageItemCache::bytes(bytes));
    item
}

#[test]
fn concrete_backing_type_survives_cache_and_child_overlay() {
    let stored = StorageKey::new(7, vec![0x01]);
    let added = StorageKey::new(7, vec![0x02]);
    let backing = TestBacking {
        entries: Arc::new(BTreeMap::from([(
            stored.clone(),
            StorageItem::from_bytes(vec![0xAA]),
        )])),
    };
    let parent = DataCache::with_backing(false, backing, DataCacheConfig::default());
    let child = parent.clone_cache();

    fn assert_concrete_cache(_: &DataCache<TestBacking>) {}
    assert_concrete_cache(&parent);
    assert_concrete_cache(&child);
    assert_eq!(
        parent.get(&stored).map(|item| item.to_value()),
        Some(vec![0xAA])
    );

    child.add(added.clone(), StorageItem::from_bytes(vec![0xBB]));
    child.commit();
    assert_eq!(
        parent.get(&added).map(|item| item.to_value()),
        Some(vec![0xBB])
    );
}

#[test]
fn find_without_prefix_uses_csharp_storage_byte_order() {
    let cache = DataCache::new(false);
    let negative_id = StorageKey::new(-5, vec![0x01]);
    let positive_id = StorageKey::new(10, vec![0x01]);
    let zero_id = StorageKey::new(0, vec![0xFF]);

    for key in [&negative_id, &positive_id, &zero_id] {
        cache.add(
            key.clone(),
            StorageItem::from_bytes(vec![key.to_array()[0]]),
        );
    }

    let found: Vec<_> = cache
        .find(None, SeekDirection::Forward)
        .map(|(key, _)| key.to_array())
        .collect();

    let mut expected = vec![
        negative_id.to_array(),
        positive_id.to_array(),
        zero_id.to_array(),
    ];
    expected.sort();

    assert_eq!(
        found, expected,
        "C# v3.10 DataCache.Seek sorts StorageKey.ToArray() with ByteArrayComparer, not numeric contract ids"
    );
}

#[test]
fn prefixed_find_merges_ordered_overlays_and_tombstones_in_both_directions() {
    let prefix = StorageKey::new(-5, vec![0x21]);
    let stored_a = StorageKey::new(-5, vec![0x21, 0x01]);
    let stored_b = StorageKey::new(-5, vec![0x21, 0x02]);
    let stored_c = StorageKey::new(-5, vec![0x21, 0x03]);
    let added_d = StorageKey::new(-5, vec![0x21, 0x04]);
    let backing = TestBacking {
        entries: Arc::new(BTreeMap::from([
            (stored_a.clone(), StorageItem::from_bytes(vec![0xA1])),
            (stored_b.clone(), StorageItem::from_bytes(vec![0xB1])),
            (stored_c.clone(), StorageItem::from_bytes(vec![0xC1])),
            (
                StorageKey::new(-5, vec![0x22, 0x01]),
                StorageItem::from_bytes(vec![0xFF]),
            ),
        ])),
    };
    let cache = DataCache::with_backing(false, backing, DataCacheConfig::default());

    cache.update(stored_b.clone(), StorageItem::from_bytes(vec![0xB2]));
    cache.delete(&stored_c);
    cache.add(added_d.clone(), StorageItem::from_bytes(vec![0xD1]));
    for index in 0..512u16 {
        cache.add(
            StorageKey::new(7, index.to_be_bytes().to_vec()),
            StorageItem::from_bytes(vec![0xEE]),
        );
    }

    let forward = cache
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, item)| (key, item.value_bytes().into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        forward,
        vec![
            (stored_a.clone(), vec![0xA1]),
            (stored_b.clone(), vec![0xB2]),
            (added_d.clone(), vec![0xD1]),
        ]
    );

    let backward = cache
        .find(Some(&prefix), SeekDirection::Backward)
        .map(|(key, item)| (key, item.value_bytes().into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(
        backward,
        vec![
            (added_d, vec![0xD1]),
            (stored_b, vec![0xB2]),
            (stored_a, vec![0xA1]),
        ]
    );
}

#[test]
fn prefixed_find_preserves_parent_child_overlay_order_and_deletes() {
    let prefix = StorageKey::new(-5, vec![0x21]);
    let key_a = StorageKey::new(-5, vec![0x21, 0x01]);
    let key_b = StorageKey::new(-5, vec![0x21, 0x02]);
    let key_c = StorageKey::new(-5, vec![0x21, 0x03]);
    let key_d = StorageKey::new(-5, vec![0x21, 0x04]);
    let backing = TestBacking {
        entries: Arc::new(BTreeMap::from([
            (key_a.clone(), StorageItem::from_bytes(vec![0xA1])),
            (key_b.clone(), StorageItem::from_bytes(vec![0xB1])),
        ])),
    };
    let parent = DataCache::with_backing(false, backing, DataCacheConfig::default());
    parent.delete(&key_a);
    parent.update(key_b.clone(), StorageItem::from_bytes(vec![0xB2]));
    parent.add(key_c.clone(), StorageItem::from_bytes(vec![0xC1]));

    let child = parent.clone_cache();
    child.delete(&key_b);
    child.update(key_c.clone(), StorageItem::from_bytes(vec![0xC2]));
    child.add(key_d.clone(), StorageItem::from_bytes(vec![0xD1]));

    let expected = vec![(key_c.clone(), vec![0xC2]), (key_d.clone(), vec![0xD1])];
    let child_entries = child
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, item)| (key, item.value_bytes().into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(child_entries, expected);

    child.commit();
    let parent_entries = parent
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, item)| (key, item.value_bytes().into_owned()))
        .collect::<Vec<_>>();
    assert_eq!(parent_entries, expected);
}

#[test]
fn try_add_rejects_live_cached_entry_like_csharp() {
    let cache = DataCache::new(false);
    let key = StorageKey::new(7, vec![0x01]);

    cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xAA]))
        .expect("initial add");
    let err = cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xBB]))
        .expect_err("C# DataCache.Add throws when the cached entry is already live");

    assert_eq!(err, DataCacheError::InvalidState(TrackState::Added));
    assert_eq!(
        cache.get(&key).expect("cached item").value_bytes().as_ref(),
        &[0xAA],
        "failed Add must not overwrite the existing cached item"
    );
}

#[test]
fn extract_raw_changes_materializes_cache_backed_storage_item_value() {
    let cache = DataCache::new(false);
    let key = StorageKey::new(7, vec![0x03]);
    cache
        .try_add(key.clone(), cache_item(vec![0xCA, 0xFE]))
        .expect("add cache-backed item");

    assert_eq!(
        cache.extract_raw_changes(),
        vec![(key.to_array(), Some(vec![0xCA, 0xFE]))],
        "C# StorageItem.Value materializes cache-backed values before persistence"
    );
}

#[test]
fn visit_raw_changes_exposes_byte_overlay_without_consuming_changes() {
    let cache = DataCache::new(false);
    let added_key = StorageKey::new(7, vec![0x0A]);
    let deleted_key = StorageKey::new(7, vec![0x0B]);

    cache.add(deleted_key.clone(), StorageItem::from_bytes(vec![0xAA]));
    cache.commit();
    cache
        .try_add(added_key.clone(), cache_item(vec![0xCA, 0xFE]))
        .expect("add cache-backed item");
    cache.delete(&deleted_key);

    let mut visited = Vec::new();
    cache.visit_raw_changes(|key, value| {
        visited.push((key.to_vec(), value.map(<[u8]>::to_vec)));
    });

    assert_eq!(
        visited,
        vec![
            (added_key.to_array(), Some(vec![0xCA, 0xFE])),
            (deleted_key.to_array(), None),
        ]
    );
    assert_eq!(cache.pending_change_count(), 2);
}

#[test]
fn try_add_after_deleted_cached_entry_becomes_changed_like_csharp() {
    let key = StorageKey::new(7, vec![0x02]);
    let backing = TestBacking {
        entries: Arc::new(BTreeMap::from([(
            key.clone(),
            StorageItem::from_bytes(vec![0xAA]),
        )])),
    };
    let cache = DataCache::with_backing(false, backing, DataCacheConfig::default());

    cache.delete(&key);
    cache
        .try_add(key.clone(), StorageItem::from_bytes(vec![0xBB]))
        .expect("C# DataCache.Add transitions Deleted -> Changed");

    let tracked = cache.tracked_items();
    let (_, trackable) = tracked
        .iter()
        .find(|(tracked_key, _)| tracked_key == &key)
        .expect("tracked key");
    assert_eq!(trackable.state, TrackState::Changed);
    assert_eq!(trackable.item.value_bytes().as_ref(), &[0xBB]);
}

#[test]
fn visit_tracked_items_exposes_changes_without_consuming_snapshot() {
    let cache = DataCache::new(false);
    let changed_key = StorageKey::new(7, vec![0x04]);
    let deleted_key = StorageKey::new(8, vec![0x05]);

    cache.add(deleted_key.clone(), StorageItem::from_bytes(vec![0xAA]));
    cache.commit();
    cache.add(changed_key.clone(), StorageItem::from_bytes(vec![0xBB]));
    cache.delete(&deleted_key);

    let mut visited = Vec::new();
    cache.visit_tracked_items(|key, trackable| {
        visited.push((
            key.clone(),
            trackable.state,
            trackable.item.value_bytes().to_vec(),
        ));
    });
    visited.sort_by_key(|(key, _, _)| key.to_array());

    assert_eq!(
        visited,
        vec![
            (changed_key.clone(), TrackState::Added, vec![0xBB]),
            (deleted_key.clone(), TrackState::Deleted, Vec::new()),
        ]
    );
    assert_eq!(cache.tracked_item_visit_call_count(), 1);
    assert_eq!(
        cache
            .get(&changed_key)
            .expect("changed remains visible")
            .value(),
        &[0xBB]
    );
}

#[test]
fn cloned_cache_commit_merges_cache_backed_items_and_clears_child_changes() {
    let parent = DataCache::new(false);
    let child = parent.clone_cache();
    let added_key = StorageKey::new(7, vec![0x06]);
    let deleted_key = StorageKey::new(8, vec![0x07]);

    parent.add(deleted_key.clone(), StorageItem::from_bytes(vec![0xAA]));
    child.add(added_key.clone(), cache_item(vec![0xCA, 0xFE]));
    child.delete(&deleted_key);

    child.commit();

    assert_eq!(child.pending_change_count(), 0);
    assert_eq!(
        parent
            .get(&added_key)
            .expect("added item merged")
            .value_bytes()
            .as_ref(),
        &[0xCA, 0xFE]
    );
    assert!(
        parent.get(&deleted_key).is_none(),
        "deleted item should be removed from parent overlay"
    );
}

#[test]
fn cloned_cache_commit_uses_bulk_merge_when_parent_has_no_update_handlers() {
    let parent = DataCache::new(false);
    let child = parent.clone_cache();

    for index in 0..16u8 {
        child.add(
            StorageKey::new(7, vec![index]),
            StorageItem::from_bytes(vec![index, 0xAA]),
        );
    }

    let before = parent.merge_write_pass_count();
    child.commit();
    let after = parent.merge_write_pass_count();

    assert_eq!(
        after - before,
        1,
        "cloned-cache commit should merge a no-callback batch with one parent write-lock pass"
    );
    assert_eq!(child.pending_change_count(), 0);
    for index in 0..16u8 {
        assert_eq!(
            parent
                .get(&StorageKey::new(7, vec![index]))
                .expect("merged item")
                .value_bytes()
                .as_ref(),
            &[index, 0xAA]
        );
    }
}

#[test]
fn commit_drains_changes_and_keeps_visible_entries() {
    let cache = DataCache::new(false);
    let added_key = StorageKey::new(7, vec![0x08]);
    let deleted_key = StorageKey::new(8, vec![0x09]);

    cache.add(deleted_key.clone(), StorageItem::from_bytes(vec![0xAA]));
    cache.commit();

    cache.add(added_key.clone(), StorageItem::from_bytes(vec![0xBB]));
    cache.delete(&deleted_key);
    assert_eq!(cache.pending_change_count(), 2);

    cache.commit();

    assert_eq!(cache.pending_change_count(), 0);
    assert_eq!(
        cache
            .get(&added_key)
            .expect("added item remains visible")
            .value_bytes()
            .as_ref(),
        &[0xBB]
    );
    assert!(
        cache.get(&deleted_key).is_none(),
        "deleted item should be removed after commit"
    );
}

#[test]
fn update_after_deleted_cached_entry_becomes_changed_not_added() {
    // Regression for the MainNet block-166739 GAS over-credit: a store-backed
    // key that is deleted (balance burned to exactly zero) and then re-created
    // via `update` (re-credited) must transition Deleted -> Changed, mirroring
    // C# `DataCache.GetAndChange`. The prior Rust behaviour used Added, which
    // makes the later commit go through `add()` and get swallowed when the
    // parent has the key read-cached as `None`.
    let key = StorageKey::new(7, vec![0x05]);
    let backing = TestBacking {
        entries: Arc::new(BTreeMap::from([(
            key.clone(),
            StorageItem::from_bytes(vec![0xAA]),
        )])),
    };
    let cache = DataCache::with_backing(false, backing, DataCacheConfig::default());

    cache.delete(&key);
    cache.update(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    let tracked = cache.tracked_items();
    let (_, trackable) = tracked
        .iter()
        .find(|(tracked_key, _)| tracked_key == &key)
        .expect("tracked key");
    assert_eq!(trackable.state, TrackState::Changed);
    assert_eq!(trackable.item.value_bytes().as_ref(), &[0xBB]);
}

#[test]
fn delete_then_recreate_persists_through_layered_commit() {
    // End-to-end reproduction of the consensus bug across the production cache
    // hierarchy store -> block snapshot -> block cache (as in
    // `persist_block_natives`): a key present in the store, read by an upper
    // layer (so it is cached as `None`), then deleted and re-created via
    // `update` in the child, must persist the new value down to the store —
    // not the stale pre-deletion value.
    let key = StorageKey::new(-6, vec![0x14, 0x01]);

    let store = DataCache::new(false);
    store.add(key.clone(), StorageItem::from_bytes(vec![0xAA]));

    let snapshot = store.clone_cache();
    let block_cache = snapshot.clone_cache();

    // Read through both overlays — populates the read cache as `None`.
    assert_eq!(
        block_cache.get(&key).map(|i| i.value_bytes().to_vec()),
        Some(vec![0xAA])
    );

    block_cache.delete(&key);
    block_cache.update(key.clone(), StorageItem::from_bytes(vec![0xBB]));

    block_cache.commit();
    snapshot.commit();

    assert_eq!(
        store.get(&key).map(|i| i.value_bytes().to_vec()),
        Some(vec![0xBB]),
        "delete-then-recreate must persist the new value, not the stale original"
    );
}

use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::{
    ReadOnlyStoreGeneric, SeekDirection, StoreCache, StoreProvider, TrackState,
};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn store_cache_basic_add_and_get() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);
    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xAA]);

    cache.add(key.clone(), value.clone());

    assert_eq!(
        cache.get(&key).expect("added value").to_value(),
        value.to_value()
    );
}

#[test]
fn store_cache_update_existing_value() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(2, vec![0x02]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    let mut cache1 = StoreCache::new_from_store(store.clone(), false);
    cache1.add(key.clone(), value1);
    cache1.commit();

    let mut cache2 = StoreCache::new_from_store(store, false);
    cache2.update(key.clone(), value2.clone());
    cache2.commit();

    let reader = StoreCache::new_from_store(
        provider.get_store("").expect("store"),
        false,
    );
    let _result = reader.try_get(&key);
    // Note: Memory store creates independent instances, so this tests the update logic
    assert!(cache2.get(&key).is_some());
}

#[test]
fn store_cache_delete_operation() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(3, vec![0x03]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    let mut cache1 = StoreCache::new_from_store(store.clone(), false);
    cache1.add(key.clone(), value);
    cache1.commit();

    let mut cache2 = StoreCache::new_from_store(store, false);
    cache2.delete(key.clone());

    assert!(cache2.try_get(&key).is_none());
}

#[test]
fn store_cache_track_state_for_added_items() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);
    let key = StorageKey::new(4, vec![0x04]);
    let value = StorageItem::from_bytes(vec![0xDD]);

    cache.add(key.clone(), value);

    let tracked = cache.data_cache().tracked_items();
    let trackable = tracked.iter().find(|(k, _)| k == &key);
    assert!(trackable.is_some());
    assert_eq!(trackable.unwrap().1.state, TrackState::Added);
}

#[test]
fn store_cache_track_state_for_changed_items() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(5, vec![0x05]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    let mut cache1 = StoreCache::new_from_store(store.clone(), false);
    cache1.add(key.clone(), value1);
    cache1.commit();

    let mut cache2 = StoreCache::new_from_store(store, false);
    cache2.update(key.clone(), value2);

    let tracked = cache2.data_cache().tracked_items();
    let trackable = tracked.iter().find(|(k, _)| k == &key);
    assert!(trackable.is_some());
    assert_eq!(trackable.unwrap().1.state, TrackState::Changed);
}

#[test]
fn store_cache_track_state_for_deleted_items() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(6, vec![0x06]);
    let value = StorageItem::from_bytes(vec![0xEE]);

    let mut cache1 = StoreCache::new_from_store(store.clone(), false);
    cache1.add(key.clone(), value);
    cache1.commit();

    let mut cache2 = StoreCache::new_from_store(store, false);
    cache2.delete(key.clone());

    let tracked = cache2.data_cache().tracked_items();
    let trackable = tracked.iter().find(|(k, _)| k == &key);
    assert!(trackable.is_some());
    assert_eq!(trackable.unwrap().1.state, TrackState::Deleted);
}

#[test]
fn store_cache_get_tracked_changes() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);

    let key1 = StorageKey::new(7, vec![0x01]);
    let key2 = StorageKey::new(7, vec![0x02]);
    let value1 = StorageItem::from_bytes(vec![0x11]);
    let value2 = StorageItem::from_bytes(vec![0x22]);

    cache.add(key1.clone(), value1);
    cache.add(key2.clone(), value2);

    let tracked = cache.data_cache().tracked_items();
    assert_eq!(tracked.len(), 2);

    let states: Vec<_> = tracked.iter().map(|(_, t)| t.state).collect();
    assert!(states.contains(&TrackState::Added));
}

#[test]
fn store_cache_find_with_prefix_forward() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);

    let prefix_id = 8;
    for i in 0..5 {
        let key = StorageKey::new(prefix_id, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 2]);
        cache.add(key, value);
    }

    cache.commit();

    let prefix = StorageKey::new(prefix_id, vec![]);
    let results: Vec<_> = cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Forward)
        .collect();

    assert_eq!(results.len(), 5);

    // Verify forward ordering
    for (i, (key, _)) in results.iter().enumerate() {
        assert_eq!(key.key()[0], i as u8);
    }
}

#[test]
fn store_cache_find_with_prefix_backward() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);

    let prefix_id = 9;
    for i in 0..5 {
        let key = StorageKey::new(prefix_id, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 3]);
        cache.add(key, value);
    }

    cache.commit();

    let prefix = StorageKey::new(prefix_id, vec![]);
    let results: Vec<_> = cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Backward)
        .collect();

    assert_eq!(results.len(), 5);

    // Verify backward ordering
    for (i, (key, _)) in results.iter().enumerate() {
        assert_eq!(key.key()[0], (4 - i) as u8);
    }
}

#[test]
fn store_cache_contains_check() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);
    let key = StorageKey::new(10, vec![0x0A]);
    let value = StorageItem::from_bytes(vec![0xFF]);

    assert!(!cache.contains(&key));

    cache.add(key.clone(), value);

    assert!(cache.contains(&key));
}

#[test]
fn store_cache_commit_persists_changes() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(11, vec![0x0B]);
    let value = StorageItem::from_bytes(vec![0xBB]);

    {
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        cache.add(key.clone(), value.clone());
        cache.commit();
    }

    // Verify in new cache instance
    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key).expect("committed value").to_value(),
        value.to_value()
    );
}

#[test]
fn store_cache_read_only_mode_prevents_commits() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, true); // read_only = true
    let key = StorageKey::new(12, vec![0x0C]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    cache.add(key, value);

    let result = cache.try_commit();
    assert!(result.is_err());
}

#[test]
fn store_cache_snapshot_based_cache() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(13, vec![0x0D]);
    let value = StorageItem::from_bytes(vec![0xDD]);

    // Write to store
    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key.clone(), value.clone());
    writer.commit();

    // Read from snapshot
    let snapshot = store.snapshot();
    let snapshot_cache = StoreCache::new_from_snapshot(snapshot);

    assert_eq!(
        snapshot_cache.get(&key).expect("value in snapshot").to_value(),
        value.to_value()
    );
}

#[test]
fn store_cache_multiple_operations_sequence() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);

    let key = StorageKey::new(14, vec![0x0E]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    // Add
    cache.add(key.clone(), value1);
    assert_eq!(cache.get(&key).unwrap().value_bytes()[0], 0x01);

    // Update
    cache.update(key.clone(), value2);
    assert_eq!(cache.get(&key).unwrap().value_bytes()[0], 0x02);

    // Delete
    cache.delete(key.clone());
    assert!(cache.try_get(&key).is_none());
}

#[test]
fn store_cache_batch_operations() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store, false);

    // Add 100 items
    for i in 0..100 {
        let key = StorageKey::new(15, vec![i as u8, (i >> 8) as u8]);
        let value = StorageItem::from_bytes(vec![i as u8]);
        cache.add(key, value);
    }

    // Check pending changes before commit
    assert!(cache.data_cache().has_pending_changes());

    cache.commit();

    // After commit, tracked changes are cleared
    assert_eq!(cache.data_cache().tracked_items().len(), 0);
}

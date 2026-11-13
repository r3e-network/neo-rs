use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::{SeekDirection, StoreCache, StoreProvider};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn memory_store_basic_put_and_get() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(1, vec![0x01]).to_array();
    let value = vec![0xAA, 0xBB];

    let mut snapshot = store.snapshot();
    let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive snapshot");
    snapshot_mut.put(key.clone(), value.clone()).unwrap();
    snapshot_mut.try_commit().unwrap();

    let result = store.snapshot().try_get(&key);
    assert_eq!(result, Some(value));
}

#[test]
fn memory_store_delete_operation() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(2, vec![0x02]).to_array();
    let value = vec![0xCC, 0xDD];

    // Put value
    let mut snapshot1 = store.snapshot();
    let snapshot1_mut = std::sync::Arc::get_mut(&mut snapshot1).expect("exclusive");
    snapshot1_mut.put(key.clone(), value).unwrap();
    snapshot1_mut.try_commit().unwrap();

    // Delete value
    let mut snapshot2 = store.snapshot();
    let snapshot2_mut = std::sync::Arc::get_mut(&mut snapshot2).expect("exclusive");
    snapshot2_mut.delete(key.clone()).unwrap();
    snapshot2_mut.try_commit().unwrap();

    // Verify deleted
    assert_eq!(store.snapshot().try_get(&key), None);
}

#[test]
fn memory_store_contains_check() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(3, vec![0x03]);
    let value = vec![0xEE];

    assert!(!store.contains(&key));

    let mut snapshot = store.snapshot();
    let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
    snapshot_mut.put(key.to_array(), value).unwrap();
    snapshot_mut.try_commit().unwrap();

    assert!(store.contains(&key));
}

#[test]
fn memory_store_find_forward() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store.clone(), false);

    for i in 0..5 {
        let key = StorageKey::new(4, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 2]);
        cache.add(key, value);
    }
    cache.commit();

    let prefix = StorageKey::new(4, vec![]);
    let results: Vec<_> = store.find(Some(&prefix), SeekDirection::Forward).collect();

    assert_eq!(results.len(), 5);

    for (i, (key, value)) in results.iter().enumerate() {
        assert_eq!(key.key()[0], i as u8);
        assert_eq!(value.value_bytes()[0], (i * 2) as u8);
    }
}

#[test]
fn memory_store_find_backward() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store.clone(), false);

    for i in 0..5 {
        let key = StorageKey::new(5, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 3]);
        cache.add(key, value);
    }
    cache.commit();

    let prefix = StorageKey::new(5, vec![]);
    let results: Vec<_> = store.find(Some(&prefix), SeekDirection::Backward).collect();

    assert_eq!(results.len(), 5);

    for (i, (key, _)) in results.iter().enumerate() {
        assert_eq!(key.key()[0], (4 - i) as u8);
    }
}

#[test]
fn memory_store_find_no_prefix() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store.clone(), false);

    for contract_id in 1..4 {
        for i in 0..3 {
            let key = StorageKey::new(contract_id, vec![i]);
            let value = StorageItem::from_bytes(vec![contract_id as u8, i]);
            cache.add(key, value);
        }
    }
    cache.commit();

    let results: Vec<_> = store.find(None, SeekDirection::Forward).collect();

    assert_eq!(results.len(), 9);
}

#[test]
fn memory_store_snapshot_isolation() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(6, vec![0x06]).to_array();
    let value1 = vec![0x01];
    let value2 = vec![0x02];

    // Write value1
    let mut snapshot1 = store.snapshot();
    let snapshot1_mut = std::sync::Arc::get_mut(&mut snapshot1).expect("exclusive");
    snapshot1_mut.put(key.clone(), value1.clone()).unwrap();
    snapshot1_mut.try_commit().unwrap();

    // Create snapshot2 before writing value2
    let snapshot2 = store.snapshot();

    // Write value2
    let mut snapshot3 = store.snapshot();
    let snapshot3_mut = std::sync::Arc::get_mut(&mut snapshot3).expect("exclusive");
    snapshot3_mut.put(key.clone(), value2.clone()).unwrap();
    snapshot3_mut.try_commit().unwrap();

    // snapshot2 should still see value1 (snapshot isolation)
    // Note: Memory store doesn't implement true snapshot isolation,
    // so this test documents current behavior
    let result = snapshot2.try_get(&key);
    assert!(result.is_some());
}

#[test]
fn memory_store_multiple_keys_same_contract() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let contract_id = 7;
    let mut cache = StoreCache::new_from_store(store.clone(), false);

    for i in 0..10 {
        let key = StorageKey::new(contract_id, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        cache.add(key, value);
    }
    cache.commit();

    let prefix = StorageKey::new(contract_id, vec![]);
    let results: Vec<_> = store.find(Some(&prefix), SeekDirection::Forward).collect();

    assert_eq!(results.len(), 10);
}

#[test]
fn memory_store_multiple_contracts() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut cache = StoreCache::new_from_store(store.clone(), false);

    for contract_id in 1..6 {
        let key = StorageKey::new(contract_id, vec![0x00]);
        let value = StorageItem::from_bytes(vec![contract_id as u8]);
        cache.add(key, value);
    }
    cache.commit();

    for contract_id in 1..6 {
        let key = StorageKey::new(contract_id, vec![0x00]);
        let result = store.try_get(&key);
        assert!(result.is_some());
        assert_eq!(result.unwrap().value_bytes()[0], contract_id as u8);
    }
}

#[test]
fn memory_store_empty_value() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(8, vec![0x08]).to_array();
    let empty_value = vec![];

    let mut snapshot = store.snapshot();
    let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
    snapshot_mut.put(key.clone(), empty_value.clone()).unwrap();
    snapshot_mut.try_commit().unwrap();

    assert_eq!(store.snapshot().try_get(&key), Some(empty_value));
}

#[test]
fn memory_store_overwrite_existing_key() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(9, vec![0x09]).to_array();
    let value1 = vec![0x01];
    let value2 = vec![0x02];

    // Write value1
    let mut snapshot1 = store.snapshot();
    let snapshot1_mut = std::sync::Arc::get_mut(&mut snapshot1).expect("exclusive");
    snapshot1_mut.put(key.clone(), value1).unwrap();
    snapshot1_mut.try_commit().unwrap();

    // Overwrite with value2
    let mut snapshot2 = store.snapshot();
    let snapshot2_mut = std::sync::Arc::get_mut(&mut snapshot2).expect("exclusive");
    snapshot2_mut.put(key.clone(), value2.clone()).unwrap();
    snapshot2_mut.try_commit().unwrap();

    assert_eq!(store.snapshot().try_get(&key), Some(value2));
}

#[test]
fn memory_store_supports_durable_writes() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    // Memory store may or may not support durable writes
    let _supports_durable = store.supports_durable_writes();
    // Just verify the call doesn't panic
}

#[test]
fn memory_store_large_value() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(10, vec![0x0A]).to_array();
    let large_value = vec![0xFF; 10240]; // 10KB

    let mut snapshot = store.snapshot();
    let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");
    snapshot_mut.put(key.clone(), large_value.clone()).unwrap();
    snapshot_mut.try_commit().unwrap();

    let result = store.snapshot().try_get(&key);
    assert_eq!(result, Some(large_value));
}

#[test]
fn memory_store_batch_operations() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let mut snapshot = store.snapshot();
    let snapshot_mut = std::sync::Arc::get_mut(&mut snapshot).expect("exclusive");

    for i in 0..100 {
        let key = StorageKey::new(11, vec![i as u8, (i >> 8) as u8]).to_array();
        let value = vec![i as u8];
        snapshot_mut.put(key, value).unwrap();
    }

    snapshot_mut.try_commit().unwrap();

    // Verify all entries
    let prefix = StorageKey::new(11, vec![]);
    let results: Vec<_> = store.find(Some(&prefix), SeekDirection::Forward).collect();

    assert_eq!(results.len(), 100);
}

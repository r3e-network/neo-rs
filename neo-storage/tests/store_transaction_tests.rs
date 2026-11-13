use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;
use neo_storage::persistence::{
    ReadOnlyStoreGeneric, StoreCache, StoreProvider, StoreTransaction, TrackState, Trackable,
};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn transaction_from_store_commits_changes() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xAA]);

    let mut tx = StoreTransaction::from_store(store.clone(), false);
    tx.cache_mut().add(key.clone(), value.clone());
    tx.commit().expect("commit transaction");

    // Verify changes are visible in store
    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key).expect("value exists").to_value(),
        value.to_value()
    );
}

#[test]
fn transaction_rollback_on_drop_does_not_persist() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xBB]);

    {
        let mut tx = StoreTransaction::from_store(store.clone(), false);
        tx.cache_mut().add(key.clone(), value.clone());
        // Drop tx without committing
    }

    // Verify changes are not in store
    let reader = StoreCache::new_from_store(store, false);
    assert!(reader.try_get(&key).is_none());
}

#[test]
fn transaction_from_snapshot_reads_snapshot_data() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(2, vec![0x02]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    // Write to store
    let mut writer = StoreCache::new_from_store(store.clone(), false);
    writer.add(key.clone(), value.clone());
    writer.commit();

    // Create transaction from snapshot
    let snapshot = store.snapshot();
    let tx = StoreTransaction::from_snapshot(snapshot);

    assert_eq!(
        tx.cache().get(&key).expect("value in snapshot").to_value(),
        value.to_value()
    );
}

#[test]
fn transaction_cache_isolation_between_transactions() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(3, vec![0x03]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    // Transaction 1 writes value1
    let mut tx1 = StoreTransaction::from_store(store.clone(), false);
    tx1.cache_mut().add(key.clone(), value1.clone());
    tx1.commit().expect("tx1 commit");

    // Transaction 2 should see value1 and update to value2
    let mut tx2 = StoreTransaction::from_store(store.clone(), false);
    assert_eq!(
        tx2.cache().get(&key).expect("tx2 sees value1").to_value(),
        value1.to_value()
    );
    tx2.cache_mut().update(key.clone(), value2.clone());
    tx2.commit().expect("tx2 commit");

    // Final verification
    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key).expect("final value").to_value(),
        value2.to_value()
    );
}

#[test]
fn transaction_delete_operation() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(4, vec![0x04]);
    let value = StorageItem::from_bytes(vec![0xDD]);

    // First add a value
    let mut tx1 = StoreTransaction::from_store(store.clone(), false);
    tx1.cache_mut().add(key.clone(), value);
    tx1.commit().expect("tx1 commit");

    // Then delete it
    let mut tx2 = StoreTransaction::from_store(store.clone(), false);
    tx2.cache_mut().delete(key.clone());
    tx2.commit().expect("tx2 commit");

    // Verify it's gone
    let reader = StoreCache::new_from_store(store, false);
    assert!(reader.try_get(&key).is_none());
}

#[test]
fn apply_tracked_items_applies_all_changes() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key1 = StorageKey::new(5, vec![0x01]);
    let key2 = StorageKey::new(5, vec![0x02]);
    let key3 = StorageKey::new(5, vec![0x03]);

    let value1 = StorageItem::from_bytes(vec![0x11]);
    let value2 = StorageItem::from_bytes(vec![0x22]);

    // Prepare tracked items
    let tracked = vec![
        (
            key1.clone(),
            Trackable {
                item: value1.clone(),
                state: TrackState::Added,
            },
        ),
        (
            key2.clone(),
            Trackable {
                item: value2.clone(),
                state: TrackState::Changed,
            },
        ),
        (
            key3.clone(),
            Trackable {
                item: StorageItem::default(),
                state: TrackState::Deleted,
            },
        ),
    ];

    let mut cache = StoreCache::new_from_store(store.clone(), false);
    neo_storage::persistence::transaction::apply_tracked_items(&mut cache, tracked);
    cache.commit();

    // Verify applied changes
    let reader = StoreCache::new_from_store(store, false);
    assert_eq!(
        reader.get(&key1).expect("key1 added").to_value(),
        value1.to_value()
    );
    assert_eq!(
        reader.get(&key2).expect("key2 changed").to_value(),
        value2.to_value()
    );
    assert!(reader.try_get(&key3).is_none(), "key3 deleted");
}

#[test]
fn transaction_read_only_prevents_commits() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let key = StorageKey::new(6, vec![0x06]);
    let value = StorageItem::from_bytes(vec![0xFF]);

    let mut tx = StoreTransaction::from_store(store, true); // read_only = true
    tx.cache_mut().add(key.clone(), value);

    // Read-only transaction should fail to commit
    let result = tx.commit();
    assert!(result.is_err(), "read-only transaction should not commit");
}

#[test]
fn transaction_multiple_operations_in_single_commit() {
    let provider = MemoryStoreProvider::new();
    let store = provider.get_store("").expect("memory store");

    let keys: Vec<StorageKey> = (0..10).map(|i| StorageKey::new(7, vec![i as u8])).collect();

    let mut tx = StoreTransaction::from_store(store.clone(), false);

    // Add multiple items
    for (i, key) in keys.iter().enumerate() {
        tx.cache_mut()
            .add(key.clone(), StorageItem::from_bytes(vec![i as u8]));
    }

    tx.commit().expect("batch commit");

    // Verify all items
    let reader = StoreCache::new_from_store(store, false);
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(
            reader.get(key).expect("item exists").value_bytes()[0],
            i as u8
        );
    }
}

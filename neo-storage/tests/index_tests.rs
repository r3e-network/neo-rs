use neo_storage::persistence::index::Index;
use neo_storage::{StorageItem, StorageKey};

#[test]
fn index_new_creates_empty() {
    let index: Index<StorageKey, StorageItem> = Index::new();
    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn index_insert_and_get() {
    let mut index = Index::new();
    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xAA]);

    index.insert(key.clone(), value.clone());

    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().to_value(), value.to_value());
}

#[test]
fn index_get_returns_none_for_missing_key() {
    let index: Index<StorageKey, StorageItem> = Index::new();
    let key = StorageKey::new(2, vec![0x02]);

    assert!(index.get(&key).is_none());
}

#[test]
fn index_contains_key() {
    let mut index = Index::new();
    let key = StorageKey::new(3, vec![0x03]);
    let value = StorageItem::from_bytes(vec![0xBB]);

    assert!(!index.contains_key(&key));

    index.insert(key.clone(), value);

    assert!(index.contains_key(&key));
}

#[test]
fn index_remove_key() {
    let mut index = Index::new();
    let key = StorageKey::new(4, vec![0x04]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    index.insert(key.clone(), value);
    assert!(index.contains_key(&key));

    let removed = index.remove(&key);
    assert!(removed.is_some());
    assert!(!index.contains_key(&key));
}

#[test]
fn index_remove_returns_value() {
    let mut index = Index::new();
    let key = StorageKey::new(5, vec![0x05]);
    let value = StorageItem::from_bytes(vec![0xDD]);

    index.insert(key.clone(), value.clone());

    let removed = index.remove(&key);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().to_value(), value.to_value());
}

#[test]
fn index_len_tracks_size() {
    let mut index = Index::new();

    assert_eq!(index.len(), 0);

    for i in 0..10 {
        let key = StorageKey::new(6, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value);
    }

    assert_eq!(index.len(), 10);
}

#[test]
fn index_clear_removes_all() {
    let mut index = Index::new();

    for i in 0..5 {
        let key = StorageKey::new(7, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 2]);
        index.insert(key, value);
    }

    assert_eq!(index.len(), 5);

    index.clear();

    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn index_overwrite_existing_key() {
    let mut index = Index::new();
    let key = StorageKey::new(8, vec![0x08]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    index.insert(key.clone(), value1);
    assert_eq!(index.get(&key).unwrap().value_bytes()[0], 0x01);

    index.insert(key.clone(), value2);
    assert_eq!(index.get(&key).unwrap().value_bytes()[0], 0x02);

    assert_eq!(index.len(), 1);
}

#[test]
fn index_iter_entries() {
    let mut index = Index::new();

    for i in 0..5 {
        let key = StorageKey::new(9, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 3]);
        index.insert(key, value);
    }

    let entries: Vec<_> = index.iter().collect();
    assert_eq!(entries.len(), 5);
}

#[test]
fn index_keys_iter() {
    let mut index = Index::new();

    for i in 0..5 {
        let key = StorageKey::new(10, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value);
    }

    let keys: Vec<_> = index.keys().collect();
    assert_eq!(keys.len(), 5);
}

#[test]
fn index_values_iter() {
    let mut index = Index::new();

    for i in 0..5 {
        let key = StorageKey::new(11, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 4]);
        index.insert(key, value);
    }

    let values: Vec<_> = index.values().collect();
    assert_eq!(values.len(), 5);
}

#[test]
fn index_get_mut() {
    let mut index = Index::new();
    let key = StorageKey::new(12, vec![0x0C]);
    let value = StorageItem::from_bytes(vec![0xEE]);

    index.insert(key.clone(), value);

    if let Some(item) = index.get_mut(&key) {
        *item = StorageItem::from_bytes(vec![0xFF]);
    }

    assert_eq!(index.get(&key).unwrap().value_bytes()[0], 0xFF);
}

#[test]
fn index_multiple_contracts() {
    let mut index = Index::new();

    for contract_id in 1..6 {
        for i in 0..3 {
            let key = StorageKey::new(contract_id, vec![i]);
            let value = StorageItem::from_bytes(vec![contract_id as u8, i]);
            index.insert(key, value);
        }
    }

    assert_eq!(index.len(), 15);
}

#[test]
fn index_entry_api_or_insert() {
    let mut index = Index::new();
    let key = StorageKey::new(13, vec![0x0D]);
    let value = StorageItem::from_bytes(vec![0x11]);

    let entry = index.entry(key.clone()).or_insert(value.clone());
    assert_eq!(entry.to_value(), value.to_value());

    // Second call should return existing value
    let new_value = StorageItem::from_bytes(vec![0x22]);
    let entry2 = index.entry(key.clone()).or_insert(new_value);
    assert_eq!(entry2.to_value(), value.to_value()); // Still original value
}

#[test]
fn index_retain_filter() {
    let mut index = Index::new();

    for i in 0..10 {
        let key = StorageKey::new(14, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value);
    }

    // Retain only even keys
    index.retain(|k, _| k.key[0] % 2 == 0);

    assert_eq!(index.len(), 5);
}

#[test]
fn index_is_empty_check() {
    let mut index = Index::new();

    assert!(index.is_empty());

    let key = StorageKey::new(15, vec![0x0F]);
    let value = StorageItem::from_bytes(vec![0xFF]);
    index.insert(key.clone(), value);

    assert!(!index.is_empty());

    index.remove(&key);

    assert!(index.is_empty());
}

#[test]
fn index_with_capacity() {
    let index: Index<StorageKey, StorageItem> = Index::with_capacity(100);

    assert_eq!(index.len(), 0);
    assert!(index.capacity() >= 100);
}

#[test]
fn index_reserve_capacity() {
    let mut index = Index::new();
    let initial_capacity = index.capacity();

    index.reserve(100);

    assert!(index.capacity() >= initial_capacity + 100);
}

#[test]
fn index_shrink_to_fit() {
    let mut index = Index::new();
    index.reserve(1000);

    for i in 0..10 {
        let key = StorageKey::new(16, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value);
    }

    index.shrink_to_fit();

    assert_eq!(index.len(), 10);
    // Capacity should be reduced but >= len
    assert!(index.capacity() >= 10);
}

#[test]
fn index_clone() {
    let mut index = Index::new();

    for i in 0..5 {
        let key = StorageKey::new(17, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 5]);
        index.insert(key, value);
    }

    let cloned = index.clone();

    assert_eq!(cloned.len(), index.len());

    for i in 0..5 {
        let key = StorageKey::new(17, vec![i]);
        assert_eq!(
            cloned.get(&key).unwrap().to_value(),
            index.get(&key).unwrap().to_value()
        );
    }
}

#[test]
fn index_extend_from_iter() {
    let mut index = Index::new();

    let entries: Vec<_> = (0..5)
        .map(|i| {
            (
                StorageKey::new(18, vec![i]),
                StorageItem::from_bytes(vec![i * 6]),
            )
        })
        .collect();

    index.extend(entries);

    assert_eq!(index.len(), 5);
}

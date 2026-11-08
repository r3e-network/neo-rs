use neo_storage::persistence::index::{BTreeIndex, HashIndex, IndexConfig, IndexType};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn btree_index_new_creates_empty() {
    let index: BTreeIndex<StorageKey, StorageItem> = BTreeIndex::new("test_index".to_string());
    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn btree_index_insert_and_get() {
    let mut index = BTreeIndex::new("test_index".to_string());
    let key = StorageKey::new(1, vec![0x01]);
    let value = StorageItem::from_bytes(vec![0xAA]);

    index.insert(key.clone(), value.clone()).expect("insert");

    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
    let values = retrieved.unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].to_value(), value.to_value());
}

#[test]
fn btree_index_get_returns_none_for_missing_key() {
    let mut index: BTreeIndex<StorageKey, StorageItem> = BTreeIndex::new("test_index".to_string());
    let key = StorageKey::new(2, vec![0x02]);

    assert!(index.get(&key).is_none());
}

#[test]
fn btree_index_remove_key() {
    let mut index = BTreeIndex::new("test_index".to_string());
    let key = StorageKey::new(4, vec![0x04]);
    let value = StorageItem::from_bytes(vec![0xCC]);

    index.insert(key.clone(), value.clone()).expect("insert");
    assert!(!index.is_empty());

    let removed = index.remove(&key, Some(&value));
    assert!(removed);
    assert!(index.is_empty());
}

#[test]
fn btree_index_len_tracks_size() {
    let mut index = BTreeIndex::new("test_index".to_string());

    assert_eq!(index.len(), 0);

    for i in 0..10 {
        let key = StorageKey::new(6, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value).expect("insert");
    }

    assert_eq!(index.len(), 10);
}

#[test]
fn btree_index_clear_removes_all() {
    let mut index = BTreeIndex::new("test_index".to_string());

    for i in 0..5 {
        let key = StorageKey::new(7, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 2]);
        index.insert(key, value).expect("insert");
    }

    assert_eq!(index.len(), 5);

    index.clear();

    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn btree_index_range_query() {
    let mut index = BTreeIndex::new("test_index".to_string());

    for i in 0..10 {
        let key = StorageKey::new(8, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 3]);
        index.insert(key, value).expect("insert");
    }

    let start = StorageKey::new(8, vec![2]);
    let end = StorageKey::new(8, vec![7]);

    let results = index.range(&start, &end);

    // Range query returns results between start and end
    assert!(!results.is_empty());
}

#[test]
fn btree_index_stats_tracking() {
    let mut index = BTreeIndex::new("test_index".to_string());

    for i in 0..5 {
        let key = StorageKey::new(9, vec![i]);
        let value = StorageItem::from_bytes(vec![i]);
        index.insert(key, value).expect("insert");
    }

    let stats = index.stats();
    assert_eq!(stats.entries, 5);
}

#[test]
fn hash_index_new_creates_empty() {
    let index: HashIndex<StorageKey, StorageItem> = HashIndex::new("hash_index".to_string());
    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn hash_index_insert_and_get() {
    let mut index = HashIndex::new("hash_index".to_string());
    let key = StorageKey::new(10, vec![0x0A]);
    let value = StorageItem::from_bytes(vec![0xBB]);

    index.insert(key.clone(), value.clone()).expect("insert");

    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
    let values = retrieved.unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].to_value(), value.to_value());
}

#[test]
fn hash_index_multiple_values_per_key() {
    let mut index = HashIndex::new("hash_index".to_string());
    let key = StorageKey::new(11, vec![0x0B]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    index.insert(key.clone(), value1).expect("insert 1");
    index.insert(key.clone(), value2).expect("insert 2");

    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
    let values = retrieved.unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn hash_index_remove_specific_value() {
    let mut index = HashIndex::new("hash_index".to_string());
    let key = StorageKey::new(12, vec![0x0C]);
    let value1 = StorageItem::from_bytes(vec![0x01]);
    let value2 = StorageItem::from_bytes(vec![0x02]);

    index.insert(key.clone(), value1.clone()).expect("insert 1");
    index.insert(key.clone(), value2.clone()).expect("insert 2");

    // Remove specific value
    let removed = index.remove(&key, Some(&value1));
    assert!(removed);

    // Should still have value2
    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
    let values = retrieved.unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].to_value(), value2.to_value());
}

#[test]
fn hash_index_clear() {
    let mut index = HashIndex::new("hash_index".to_string());

    for i in 0..10 {
        let key = StorageKey::new(13, vec![i]);
        let value = StorageItem::from_bytes(vec![i * 4]);
        index.insert(key, value).expect("insert");
    }

    assert_eq!(index.len(), 10);

    index.clear();

    assert_eq!(index.len(), 0);
    assert!(index.is_empty());
}

#[test]
fn index_with_config() {
    let config = IndexConfig {
        name: "custom_index".to_string(),
        index_type: IndexType::BTree,
        unique: true,
        case_sensitive: true,
    };

    let mut index: BTreeIndex<StorageKey, StorageItem> = BTreeIndex::with_config(config);

    let key = StorageKey::new(14, vec![0x0E]);
    let value = StorageItem::from_bytes(vec![0xEE]);

    index.insert(key.clone(), value.clone()).expect("insert");

    let retrieved = index.get(&key);
    assert!(retrieved.is_some());
}

#[test]
fn btree_index_is_empty_check() {
    let mut index = BTreeIndex::new("test_index".to_string());

    assert!(index.is_empty());

    let key = StorageKey::new(15, vec![0x0F]);
    let value = StorageItem::from_bytes(vec![0xFF]);
    index.insert(key.clone(), value.clone()).expect("insert");

    assert!(!index.is_empty());

    index.remove(&key, Some(&value));

    assert!(index.is_empty());
}

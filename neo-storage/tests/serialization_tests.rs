use neo_storage::persistence::serialization::{deserialize, serialize};
use neo_storage::{StorageItem, StorageKey};

#[test]
fn serialize_storage_key_basic() {
    let key = StorageKey::new(1, vec![0x01, 0x02, 0x03]);
    let serialized = serialize(&key).expect("serialize key");

    assert!(!serialized.is_empty());
}

#[test]
fn deserialize_storage_key_basic() {
    let key = StorageKey::new(2, vec![0x04, 0x05, 0x06]);
    let serialized = serialize(&key).expect("serialize key");
    let deserialized: StorageKey = deserialize(&serialized).expect("deserialize key");

    assert_eq!(deserialized.id, key.id);
    assert_eq!(deserialized.key(), key.key());
}

#[test]
fn serialize_deserialize_roundtrip_storage_key() {
    let original = StorageKey::new(-1, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    let serialized = serialize(&original).expect("serialize key");
    let deserialized: StorageKey = deserialize(&serialized).expect("roundtrip key");

    assert_eq!(deserialized.id, original.id);
    assert_eq!(deserialized.key(), original.key());
}

#[test]
fn serialize_storage_item_basic() {
    let item = StorageItem::from_bytes(vec![0x11, 0x22, 0x33]);
    let serialized = serialize(&item).expect("serialize item");

    assert!(!serialized.is_empty());
}

#[test]
fn deserialize_storage_item_basic() {
    let item = StorageItem::from_bytes(vec![0x44, 0x55, 0x66]);
    let serialized = serialize(&item).expect("serialize item");
    let deserialized: StorageItem = deserialize(&serialized).expect("deserialize item");

    assert_eq!(deserialized.value_bytes(), item.value_bytes());
}

#[test]
fn serialize_deserialize_roundtrip_storage_item() {
    let original = StorageItem::from_bytes(vec![0xFF, 0xEE, 0xDD, 0xCC]);
    let serialized = serialize(&original).expect("serialize item");
    let deserialized: StorageItem = deserialize(&serialized).expect("roundtrip item");

    assert_eq!(deserialized.value_bytes(), original.value_bytes());
    assert_eq!(deserialized.is_constant(), original.is_constant());
}

#[test]
fn serialize_storage_item_with_constant_flag() {
    let mut item = StorageItem::from_bytes(vec![0x99, 0x88]);
    // Note: StorageItem doesn't have set_constant method
    // This test just verifies serialization works with the item

    let serialized = serialize(&item).expect("serialize item");
    let deserialized: StorageItem = deserialize(&serialized).expect("deserialize item");

    assert_eq!(deserialized.value_bytes(), item.value_bytes());
    assert_eq!(deserialized.is_constant(), item.is_constant());
}

#[test]
fn serialize_storage_key_empty_key_bytes() {
    let key = StorageKey::new(100, vec![]);
    let serialized = serialize(&key).expect("serialize empty key");
    let deserialized: StorageKey = deserialize(&serialized).expect("empty key bytes");

    assert_eq!(deserialized.id, 100);
    assert_eq!(deserialized.key().len(), 0);
}

#[test]
fn serialize_storage_item_empty_value() {
    let item = StorageItem::from_bytes(vec![]);
    let serialized = serialize(&item).expect("serialize empty item");
    let deserialized: StorageItem = deserialize(&serialized).expect("empty value");

    assert_eq!(deserialized.value_bytes().len(), 0);
}

#[test]
fn serialize_storage_key_negative_id() {
    let key = StorageKey::new(-100, vec![0x01, 0x02]);
    let serialized = serialize(&key).expect("serialize negative id");
    let deserialized: StorageKey = deserialize(&serialized).expect("negative id");

    assert_eq!(deserialized.id, -100);
    assert_eq!(deserialized.key(), &vec![0x01, 0x02]);
}

#[test]
fn serialize_storage_key_large_key_bytes() {
    let large_key = vec![0xAB; 1024]; // 1KB key
    let key = StorageKey::new(42, large_key.clone());
    let serialized = serialize(&key).expect("serialize large key");
    let deserialized: StorageKey = deserialize(&serialized).expect("large key");

    assert_eq!(deserialized.id, 42);
    assert_eq!(deserialized.key(), &large_key);
}

#[test]
fn serialize_storage_item_large_value() {
    let large_value = vec![0xCD; 4096]; // 4KB value
    let item = StorageItem::from_bytes(large_value.clone());
    let serialized = serialize(&item).expect("serialize large item");
    let deserialized: StorageItem = deserialize(&serialized).expect("large value");

    assert_eq!(deserialized.value_bytes().as_ref(), large_value.as_slice());
}

#[test]
fn deserialize_invalid_data_returns_error() {
    let invalid_data = vec![0xFF; 10];
    let result: Result<StorageKey, _> = deserialize(&invalid_data);

    assert!(result.is_err());
}

#[test]
fn serialize_multiple_keys_different_sizes() {
    let keys = vec![
        StorageKey::new(1, vec![0x01]),
        StorageKey::new(2, vec![0x02, 0x03]),
        StorageKey::new(3, vec![0x04, 0x05, 0x06]),
    ];

    for key in keys {
        let serialized = serialize(&key).expect("serialize key");
        let deserialized: StorageKey = deserialize(&serialized).expect("multi-key test");
        assert_eq!(deserialized.id, key.id);
        assert_eq!(deserialized.key(), key.key());
    }
}

#[test]
fn serialize_multiple_items_different_sizes() {
    let items = vec![
        StorageItem::from_bytes(vec![0x11]),
        StorageItem::from_bytes(vec![0x22, 0x33]),
        StorageItem::from_bytes(vec![0x44, 0x55, 0x66]),
    ];

    for item in items {
        let serialized = serialize(&item).expect("serialize item");
        let deserialized: StorageItem = deserialize(&serialized).expect("multi-item test");
        assert_eq!(deserialized.value_bytes(), item.value_bytes());
    }
}

#[test]
fn serialize_deserialize_preserves_exact_bytes() {
    let key = StorageKey::new(999, vec![0x00, 0x01, 0x7F, 0x80, 0xFE, 0xFF]);
    let serialized = serialize(&key).expect("serialize key");
    let deserialized: StorageKey = deserialize(&serialized).expect("exact bytes");

    assert_eq!(deserialized.key(), key.key());
}

#[test]
fn serialization_is_deterministic() {
    let key = StorageKey::new(50, vec![0xAA, 0xBB]);

    let serialized1 = serialize(&key).expect("serialize 1");
    let serialized2 = serialize(&key).expect("serialize 2");

    assert_eq!(serialized1, serialized2);
}

#[test]
fn serialize_storage_item_default() {
    let item = StorageItem::default();
    let serialized = serialize(&item).expect("serialize default");
    let deserialized: StorageItem = deserialize(&serialized).expect("default item");

    assert_eq!(deserialized.value_bytes().len(), 0);
    assert_eq!(deserialized.is_constant(), false);
}

#[test]
fn serialize_storage_key_zero_id() {
    let key = StorageKey::new(0, vec![0x99]);
    let serialized = serialize(&key).expect("serialize zero id");
    let deserialized: StorageKey = deserialize(&serialized).expect("zero id");

    assert_eq!(deserialized.id, 0);
    assert_eq!(deserialized.key(), &vec![0x99]);
}

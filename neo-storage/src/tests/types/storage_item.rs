use super::*;

fn cache_item(bytes: Vec<u8>) -> StorageItem {
    let mut item = StorageItem::new();
    item.set_cache(StorageItemCache::bytes(bytes));
    item
}

#[test]
fn test_storage_item_creation() {
    let item = StorageItem::from_bytes(vec![0xAA, 0xBB]);
    assert_eq!(item.value(), &[0xAA, 0xBB]);
}

#[test]
fn test_storage_item_get_value() {
    let item = StorageItem::from_bytes(vec![0x01, 0x02, 0x03]);
    assert_eq!(item.to_value(), vec![0x01, 0x02, 0x03]);
}

#[test]
fn test_storage_item_set_value() {
    let mut item = StorageItem::from_bytes(vec![0x01]);
    item.set_value(vec![0x02, 0x03]);
    assert_eq!(item.value(), &[0x02, 0x03]);
}

#[test]
fn test_storage_item_size() {
    let item = StorageItem::from_bytes(vec![0x01, 0x02, 0x03, 0x04]);
    assert_eq!(item.size(), 5);
}

#[test]
fn test_storage_item_size_uses_var_size_prefix() {
    let compact = StorageItem::from_bytes(vec![0; 252]);
    let expanded = StorageItem::from_bytes(vec![0; 253]);

    assert_eq!(compact.size(), 253);
    assert_eq!(expanded.size(), 256);
}

#[test]
fn test_storage_item_default() {
    let item = StorageItem::default();
    let empty: &[u8] = &[];
    assert_eq!(item.value(), empty);
    assert_eq!(item.size(), 1);
}

#[test]
fn test_storage_item_from_bytes() {
    let item = StorageItem::from_bytes(vec![0xAA, 0xBB]);
    assert_eq!(item.value(), &[0xAA, 0xBB]);
}

#[test]
fn test_storage_item_clone() {
    let item1 = StorageItem::from_bytes(vec![0x01, 0x02]);
    let item2 = item1.clone();
    assert_eq!(item1, item2);
}

#[test]
fn test_storage_item_equality() {
    let item1 = StorageItem::from_bytes(vec![0x01]);
    let item2 = StorageItem::from_bytes(vec![0x01]);
    let item3 = StorageItem::from_bytes(vec![0x02]);

    assert_eq!(item1, item2);
    assert_ne!(item1, item3);
}

#[test]
fn test_storage_item_equality_uses_materialized_cache_value() {
    assert_eq!(
        cache_item(vec![0x01, 0x02]),
        StorageItem::from_bytes(vec![0x01, 0x02])
    );
    assert_ne!(cache_item(vec![0x01]), StorageItem::from_bytes(vec![]));
}

#[test]
fn test_storage_item_debug() {
    let item = StorageItem::from_bytes(vec![0x01]);
    let debug_str = format!("{:?}", item);
    assert!(debug_str.contains("StorageItem"));
}

#[test]
fn test_storage_item_from_vec() {
    let item: StorageItem = vec![0x01, 0x02].into();
    assert_eq!(item.value(), &[0x01, 0x02]);
}

#[test]
fn test_storage_item_from_slice() {
    let bytes: &[u8] = &[0x01, 0x02];
    let item: StorageItem = bytes.into();
    assert_eq!(item.value(), &[0x01, 0x02]);
}

#[test]
fn test_serde_storage_item() {
    let item = StorageItem::from_bytes(vec![0xAA, 0xBB]);
    let serialized = serde_json::to_string(&item).unwrap();
    let deserialized: StorageItem = serde_json::from_str(&serialized).unwrap();
    assert_eq!(item, deserialized);
}

#[test]
fn test_storage_item_to_storage_bytes_is_raw_value() {
    // C# StorageItem.Serialize writes the raw value bytes only (no flag).
    let item = StorageItem::from_bytes(vec![0xAA, 0xBB, 0xCC]);
    assert_eq!(item.to_storage_bytes(), vec![0xAA, 0xBB, 0xCC]);
}

#[test]
fn test_storage_item_from_storage_bytes_is_raw_value() {
    let data = vec![0xAA, 0xBB];
    let item = StorageItem::from_storage_bytes(&data).unwrap();
    assert_eq!(item.value(), &[0xAA, 0xBB]);
}

#[test]
fn test_storage_item_from_storage_bytes_empty() {
    let data: &[u8] = &[];
    let item = StorageItem::from_storage_bytes(data).unwrap();
    let empty: &[u8] = &[];
    assert_eq!(item.value(), empty);
}

#[test]
fn test_storage_item_storage_size() {
    let item = StorageItem::from_bytes(vec![0x01, 0x02, 0x03]);
    assert_eq!(item.storage_size(), 3);
}

#[test]
fn test_storage_item_storage_size_empty() {
    let item = StorageItem::from_bytes(vec![]);
    assert_eq!(item.storage_size(), 0);
}

#[test]
fn test_storage_item_roundtrip() {
    let original = StorageItem::from_bytes(vec![0x00, 0xFF, 0x12, 0x34]);
    let bytes = original.to_storage_bytes();
    let restored = StorageItem::from_storage_bytes(&bytes).unwrap();
    assert_eq!(original.value(), restored.value());
}

#[test]
fn test_storage_item_roundtrip_large() {
    let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
    let original = StorageItem::from_bytes(large_data.clone());
    let bytes = original.to_storage_bytes();
    let restored = StorageItem::from_storage_bytes(&bytes).unwrap();
    assert_eq!(original.value(), restored.value());
    assert_eq!(original.storage_size(), 1000);
}

#[test]
fn test_storage_item_istorage_value_trait_object() {
    fn use_storage_value<V: StorageValue>(value: &V) -> usize {
        value.storage_size()
    }

    let item = StorageItem::from_bytes(vec![0x01, 0x02, 0x03]);
    assert_eq!(use_storage_value(&item), 3);
}

#[test]
fn test_value_bytes_borrowed() {
    let item = StorageItem::from_bytes(vec![0x01, 0x02]);
    match item.value_bytes() {
        Cow::Borrowed(_) => {}
        Cow::Owned(_) => panic!("expected borrowed"),
    }
}

#[test]
fn test_seal_no_cache() {
    let mut item = StorageItem::from_bytes(vec![0x01]);
    item.seal();
    assert_eq!(item.value(), &[0x01]);
}

#[test]
fn test_from_replica() {
    let item1 = StorageItem::from_bytes(vec![0xAA]);
    let mut item2 = StorageItem::new();
    item2.from_replica(&item1);
    assert_eq!(item2.value(), &[0xAA]);
}

#[test]
fn test_serialize() {
    let item = StorageItem::from_bytes(vec![0x01, 0x02]);
    assert_eq!(item.to_storage_bytes(), vec![0x01, 0x02]);
}

#[test]
fn test_deserialize_from_bytes() {
    let mut item = StorageItem::new();
    item.deserialize_from_bytes(&[0xAA, 0xBB]);
    assert_eq!(item.value(), &[0xAA, 0xBB]);
}

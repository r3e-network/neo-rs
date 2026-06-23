use super::*;

#[test]
fn test_storage_key_creation() {
    let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02, 0x03]);
}

#[test]
fn test_storage_key_create() {
    let key = StorageKey::create(-4, 0x05);
    assert_eq!(key.id(), -4);
    assert_eq!(key.key(), &[0x05]);
}

#[test]
fn test_storage_key_create_with_byte() {
    let key = StorageKey::create_with_byte(-1, 0x10, 0x42);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x10, 0x42]);
}

#[test]
fn test_storage_key_create_with_uint160() {
    let hash = UInt160::zero();
    let key = StorageKey::create_with_uint160(-1, 0x14, &hash);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 21);
    assert_eq!(key.key()[0], 0x14);
}

#[test]
fn test_storage_key_create_with_uint256() {
    let hash = UInt256::zero();
    let key = StorageKey::create_with_uint256(-2, 0x15, &hash);
    assert_eq!(key.id(), -2);
    assert_eq!(key.key().len(), 33);
    assert_eq!(key.key()[0], 0x15);
}

#[test]
fn test_storage_key_create_with_int32() {
    let key = StorageKey::create_with_int32(-1, 0x20, 0x12345678);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 5);
    assert_eq!(key.key()[0], 0x20);
    assert_eq!(&key.key()[1..], &[0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn test_storage_key_create_with_int64() {
    let key = StorageKey::create_with_int64(-1, 0x21, 0x123456789ABCDEF0u64 as i64);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key().len(), 9);
    assert_eq!(key.key()[0], 0x21);
}

#[test]
fn test_storage_key_create_with_bytes() {
    let content = vec![0xAA, 0xBB, 0xCC];
    let key = StorageKey::create_with_bytes(-1, 0x30, &content);
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x30, 0xAA, 0xBB, 0xCC]);
}

#[test]
fn test_storage_key_create_search_prefix() {
    let prefix = StorageKey::create_search_prefix(-1, &[0x14]);
    assert_eq!(prefix.len(), 5);
    assert_eq!(&prefix[..4], &(-1i32).to_le_bytes());
    assert_eq!(prefix[4], 0x14);
}

#[test]
fn test_storage_key_ordering_matches_serialized_bytes() {
    let key1 = StorageKey::new(-1, vec![0x01]);
    let key2 = StorageKey::new(-1, vec![0x02]);
    let key3 = StorageKey::new(0, vec![0x01]);

    assert!(key1 < key2);
    assert!(
        key3 < key1,
        "C# DataCache orders StorageKey.ToArray() with ByteArrayComparer, so little-endian id bytes drive cross-contract ordering"
    );
}

#[test]
fn test_storage_key_ordering_same_id() {
    let key1 = StorageKey::new(5, vec![0x01]);
    let key2 = StorageKey::new(5, vec![0x02]);
    let key3 = StorageKey::new(5, vec![0x01]);

    assert!(key1 < key2);
    assert_eq!(key1, key3);
    assert!(key2 > key1);
}

#[test]
fn test_storage_key_ordering_different_id() {
    let key1 = StorageKey::new(-5, vec![0xFF]);
    let key2 = StorageKey::new(10, vec![0x00]);

    assert!(key2 < key1);
}

#[test]
fn storage_key_ord_matches_csharp_v310_byte_array_comparer() {
    let mut keys = [
        StorageKey::new(-5, vec![0x01]),
        StorageKey::new(10, vec![0x01]),
        StorageKey::new(0, vec![0xFF]),
        StorageKey::new(-1, vec![0x00]),
    ];

    let mut expected: Vec<_> = keys.iter().map(StorageKey::to_array).collect();
    expected.sort();

    keys.sort();

    assert_eq!(
        keys.iter().map(StorageKey::to_array).collect::<Vec<_>>(),
        expected,
        "C# v3.10 DataCache.Seek orders p.Key.ToArray() using ByteArrayComparer.SequenceCompareTo"
    );
}

#[test]
fn test_storage_key_to_array() {
    let key = StorageKey::new(-1, vec![0xAA, 0xBB]);
    let array = key.to_array();
    assert_eq!(&array[..4], &(-1i32).to_le_bytes());
    assert_eq!(&array[4..], &[0xAA, 0xBB]);
}

#[test]
fn test_storage_key_from_bytes() {
    let bytes = vec![0x01, 0x02, 0x03, 0x04, 0xAA, 0xBB];
    let key = StorageKey::from_bytes(&bytes);
    let expected_id = i32::from_le_bytes([0x01, 0x02, 0x03, 0x04]);
    assert_eq!(key.id(), expected_id);
    assert_eq!(key.key(), &[0xAA, 0xBB]);
}

#[test]
fn test_storage_key_equality_and_hash_ignore_cached_bytes() {
    use std::collections::HashSet;

    let constructed = StorageKey::new(-1, vec![0xAA, 0xBB]);
    let roundtrip = StorageKey::from_bytes(&constructed.to_array());

    assert_eq!(constructed, roundtrip);

    let mut keys = HashSet::new();
    keys.insert(constructed);
    assert!(keys.contains(&roundtrip));
}

#[test]
fn test_storage_key_suffix() {
    let key = StorageKey::new(-1, vec![0x01, 0x02]);
    assert_eq!(key.suffix(), key.key());
}

#[test]
fn test_storage_key_length() {
    let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
    assert_eq!(key.length(), 8);
}

#[test]
fn test_storage_key_clone() {
    let key1 = StorageKey::new(-1, vec![0x01, 0x02]);
    let key2 = key1.clone();
    assert_eq!(key1, key2);
}

#[test]
fn test_storage_key_hash_set() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    let key1 = StorageKey::new(-1, vec![0x01]);
    let key2 = StorageKey::new(-1, vec![0x01]);
    let key3 = StorageKey::new(-1, vec![0x02]);

    set.insert(key1.clone());
    assert!(set.contains(&key2));
    assert!(!set.contains(&key3));
}

#[test]
fn test_storage_key_get_hash_code() {
    let key = StorageKey::new(-1, vec![0x14, 0xAA, 0xBB]);
    let hash1 = key.hash_code();
    let hash2 = key.hash_code();
    assert_eq!(hash1, hash2);
}

#[test]
fn test_storage_key_display_empty() {
    let key = StorageKey::new(-1, vec![]);
    let display = format!("{}", key);
    assert!(display.contains("Id = -1"));
    assert!(display.contains("Key = {}"));
}

#[test]
fn test_storage_key_display_with_prefix() {
    let key = StorageKey::new(-1, vec![0x14, 0xAA, 0xBB]);
    let display = format!("{}", key);
    assert!(display.contains("Id = -1"));
    assert!(display.contains("Prefix = 0x14"));
}

#[test]
fn test_storage_key_debug() {
    let key = StorageKey::new(-1, vec![0x01]);
    let debug_str = format!("{:?}", key);
    assert!(debug_str.contains("StorageKey"));
}

#[test]
fn test_storage_key_from_vec() {
    let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02];
    let key: StorageKey = bytes.into();
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02]);
}

#[test]
fn test_storage_key_from_slice() {
    let bytes: &[u8] = &[0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0x02];
    let key: StorageKey = bytes.into();
    assert_eq!(key.id(), -1);
    assert_eq!(key.key(), &[0x01, 0x02]);
}

#[test]
fn test_serde_storage_key() {
    let key = StorageKey::new(-1, vec![0x01, 0x02]);
    let serialized = serde_json::to_string(&key).unwrap();
    let deserialized: StorageKey = serde_json::from_str(&serialized).unwrap();
    assert_eq!(key.id, deserialized.id);
    assert_eq!(key.key, deserialized.key);
}

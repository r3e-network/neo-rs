use super::*;
use crate::{StorageItem, StorageKey};

// ============================================================================
// StorageConfig Tests
// ============================================================================

#[test]
fn storage_config_default_values() {
    let config = StorageConfig::default();
    assert_eq!(config.path, PathBuf::from("./data"));
    assert!(!config.read_only);
}

#[test]
fn storage_config_exposes_mdbx_geometry_knobs() {
    let config = StorageConfig::default();

    assert_eq!(config.mdbx_geometry_upper_bytes, None);
    assert_eq!(config.mdbx_geometry_growth_bytes, None);
    assert_eq!(config.mdbx_max_readers, None);
}

#[test]
fn storage_config_clone() {
    let config = StorageConfig::default();
    let cloned = config.clone();
    assert_eq!(config.path, cloned.path);
    assert_eq!(
        config.mdbx_geometry_upper_bytes,
        cloned.mdbx_geometry_upper_bytes
    );
}

// ============================================================================
// StorageKey Tests
// ============================================================================

#[test]
fn storage_key_new_creates_key() {
    let key = StorageKey::new(42, vec![1, 2, 3]);
    assert_eq!(key.id, 42);
    assert_eq!(key.suffix(), &[1, 2, 3]);
}

#[test]
fn storage_key_to_bytes_and_from_bytes_roundtrip() {
    let original = StorageKey::new(12345, vec![0xAB, 0xCD, 0xEF]);
    let bytes = original.to_array();
    let restored = StorageKey::from_bytes(&bytes);

    assert_eq!(original.id, restored.id);
    assert_eq!(original.suffix(), restored.suffix());
}

#[test]
fn storage_key_from_bytes_exact_four_bytes() {
    let bytes = vec![1, 0, 0, 0]; // id = 1 in little-endian
    let key = StorageKey::from_bytes(&bytes);
    assert_eq!(key.id, 1);
    assert!(key.suffix().is_empty());
}

#[test]
fn storage_key_equality() {
    let key1 = StorageKey::new(1, vec![1, 2, 3]);
    let key2 = StorageKey::new(1, vec![1, 2, 3]);
    let key3 = StorageKey::new(2, vec![1, 2, 3]);

    assert_eq!(key1, key2);
    assert_ne!(key1, key3);
}

#[test]
fn storage_key_hash_consistency() {
    use std::collections::HashSet;

    let key1 = StorageKey::new(1, vec![1, 2, 3]);
    let key2 = StorageKey::new(1, vec![1, 2, 3]);

    let mut set = HashSet::new();
    set.insert(key1);
    assert!(set.contains(&key2));
}

// ============================================================================
// StorageItem Tests
// ============================================================================

#[test]
fn storage_item_new_creates_item() {
    let item = StorageItem::from_bytes(vec![1, 2, 3]);
    assert_eq!(item.to_value(), vec![1, 2, 3]);
}

#[test]
fn storage_item_default_is_empty() {
    let item = StorageItem::new();
    assert!(item.to_value().is_empty());
}

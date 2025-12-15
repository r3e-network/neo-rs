//! Storage types for Neo blockchain.
//!
//! This module provides the core storage types that match the C# Neo implementation:
//! - `StorageKey`: Keys for contract storage with contract ID and key bytes
//! - `StorageItem`: Values stored in contract storage
//! - `SeekDirection`: Direction for storage iteration
//! - `TrackState`: Cache tracking states for storage entries

use crate::hash_utils::{default_xx_hash3_seed, hash_code_combine_i32, xx_hash3_32};
use neo_primitives::{IStorageValue, StorageValueError, StorageValueResult, UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Direction for seeking in storage.
///
/// Matches C# Neo.Persistence.SeekDirection exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(i8)]
pub enum SeekDirection {
    /// Indicates that the search should be performed in ascending order.
    #[default]
    Forward = 1,
    /// Indicates that the search should be performed in descending order.
    Backward = -1,
}

/// Track state for cached storage items.
///
/// Matches C# Neo.Persistence.TrackState exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum TrackState {
    /// Indicates that the entry has been loaded from the underlying storage, but has not been modified.
    #[default]
    None = 0,
    /// Indicates that this is a newly added record.
    Added = 1,
    /// Indicates that the entry has been loaded from the underlying storage, and has been modified.
    Changed = 2,
    /// Indicates that the entry should be deleted from the underlying storage when committing.
    Deleted = 3,
    /// Indicates that the entry was not found in the underlying storage.
    NotFound = 4,
}

/// Storage key for Neo blockchain.
///
/// Represents the keys in contract storage, matching C# Neo.SmartContract.StorageKey exactly.
/// Combines a contract ID with a key suffix to form a unique storage key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    /// Contract ID (native contracts use negative IDs).
    pub id: i32,
    /// Key suffix (variable length).
    key: Vec<u8>,
    /// Cached full key (optional).
    #[serde(skip)]
    cache: Option<Vec<u8>>,
}

impl StorageKey {
    /// Prefix length: sizeof(i32) + sizeof(u8) = 5 bytes.
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Creates a new storage key.
    pub fn new(id: i32, key: Vec<u8>) -> Self {
        Self { id, key, cache: None }
    }

    /// Helper to construct key bytes with prefix.
    #[inline]
    fn storage_key(prefix: u8, suffix: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + suffix.len());
        key.push(prefix);
        key.extend_from_slice(suffix);
        key
    }

    /// Returns the total length of the serialized key.
    pub fn length(&self) -> usize {
        if let Some(ref cache) = self.cache {
            cache.len()
        } else {
            Self::PREFIX_LENGTH + self.key.len()
        }
    }

    /// Creates a storage key with a single-byte prefix.
    pub fn create(id: i32, prefix: u8) -> Self {
        let key = Self::storage_key(prefix, &[]);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and single byte content.
    pub fn create_with_byte(id: i32, prefix: u8, content: u8) -> Self {
        let key = Self::storage_key(prefix, &[content]);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and UInt160 hash.
    pub fn create_with_uint160(id: i32, prefix: u8, hash: &UInt160) -> Self {
        let key = Self::storage_key(prefix, &hash.as_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and UInt256 hash.
    pub fn create_with_uint256(id: i32, prefix: u8, hash: &UInt256) -> Self {
        let key = Self::storage_key(prefix, &hash.as_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix, UInt256 hash, and UInt160 signer.
    pub fn create_with_uint256_uint160(
        id: i32,
        prefix: u8,
        hash: &UInt256,
        signer: &UInt160,
    ) -> Self {
        let mut suffix = hash.as_bytes().to_vec();
        suffix.extend_from_slice(&signer.as_bytes());
        let key = Self::storage_key(prefix, &suffix);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and i32 value (big endian).
    pub fn create_with_int32(id: i32, prefix: u8, big_endian: i32) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and i64 value (big endian).
    pub fn create_with_int64(id: i32, prefix: u8, big_endian: i64) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and byte content.
    pub fn create_with_bytes(id: i32, prefix: u8, content: &[u8]) -> Self {
        let key = Self::storage_key(prefix, content);
        Self::new(id, key)
    }

    /// Creates a search prefix for iterating contract storage.
    pub fn create_search_prefix(id: i32, prefix: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + prefix.len()];
        buffer[..4].copy_from_slice(&id.to_le_bytes());
        buffer[4..].copy_from_slice(prefix);
        buffer
    }

    /// Returns the contract ID.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Returns the key suffix.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Alias for key() - returns the suffix portion (excluding contract ID).
    pub fn suffix(&self) -> &[u8] {
        &self.key
    }

    /// Converts the storage key to a byte array for storage.
    pub fn to_array(&self) -> Vec<u8> {
        if let Some(ref cache) = self.cache {
            cache.clone()
        } else {
            self.build()
        }
    }

    /// Returns the hash code using the same algorithm as the C# implementation.
    pub fn get_hash_code(&self) -> i32 {
        let seed = default_xx_hash3_seed();
        let suffix_hash = xx_hash3_32(&self.key, seed);
        hash_code_combine_i32(self.id, suffix_hash)
    }

    /// Builds the full key bytes.
    fn build(&self) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + self.key.len()];
        buffer[..4].copy_from_slice(&self.id.to_le_bytes());
        buffer[4..].copy_from_slice(&self.key);
        buffer
    }

    /// Creates a storage key from raw bytes.
    pub fn from_bytes(cache: &[u8]) -> Self {
        if cache.len() < 4 {
            return Self { id: 0, key: cache.to_vec(), cache: Some(cache.to_vec()) };
        }
        let id = i32::from_le_bytes([cache[0], cache[1], cache[2], cache[3]]);
        let key = cache[4..].to_vec();
        Self {
            id,
            key,
            cache: Some(cache.to_vec()),
        }
    }
}

impl PartialOrd for StorageKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StorageKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.id.cmp(&other.id) {
            std::cmp::Ordering::Equal => self.key.cmp(&other.key),
            other => other,
        }
    }
}

impl From<Vec<u8>> for StorageKey {
    fn from(value: Vec<u8>) -> Self {
        Self::from_bytes(&value)
    }
}

impl From<&[u8]> for StorageKey {
    fn from(value: &[u8]) -> Self {
        Self::from_bytes(value)
    }
}

impl fmt::Display for StorageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            write!(f, "Id = {}, Key = {{}}", self.id)
        } else {
            write!(
                f,
                "Id = {}, Prefix = 0x{:02x}, Key = {{ {} }}",
                self.id,
                self.key[0],
                self.key[1..]
                    .iter()
                    .map(|b| format!("0x{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

/// Storage item for Neo blockchain.
///
/// Represents a value stored in the blockchain state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageItem {
    /// The stored value.
    value: Vec<u8>,
    /// Whether this item is constant (cannot be modified).
    is_constant: bool,
}

impl StorageItem {
    /// Creates a new storage item.
    pub fn new(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: false,
        }
    }

    /// Creates a storage item from bytes.
    pub fn from_bytes(value: Vec<u8>) -> Self {
        Self::new(value)
    }

    /// Creates a constant storage item.
    pub fn constant(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: true,
        }
    }

    /// Returns a clone of the stored value.
    pub fn get_value(&self) -> Vec<u8> {
        self.value.clone()
    }

    /// Returns a reference to the stored value.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Sets the stored value.
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = value;
    }

    /// Returns whether this item is constant.
    pub fn is_constant(&self) -> bool {
        self.is_constant
    }

    /// Returns the size of the stored value.
    pub fn size(&self) -> usize {
        self.value.len()
    }
}

impl Default for StorageItem {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl From<Vec<u8>> for StorageItem {
    fn from(value: Vec<u8>) -> Self {
        Self::from_bytes(value)
    }
}

impl From<&[u8]> for StorageItem {
    fn from(value: &[u8]) -> Self {
        Self::from_bytes(value.to_vec())
    }
}

// ============ IStorageValue Implementation ============

/// Implement `IStorageValue` trait from neo-primitives.
///
/// This allows `StorageItem` to be used with generic storage abstractions
/// that require the `IStorageValue` trait, breaking the circular dependency
/// between neo-storage and neo-vm.
///
/// # Serialization Format
///
/// The storage format is:
/// - 1 byte: `is_constant` flag (0x00 or 0x01)
/// - N bytes: raw value data
///
/// This matches the expected format for neo-core compatibility.
impl IStorageValue for StorageItem {
    fn to_storage_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + self.value.len());
        bytes.push(if self.is_constant { 0x01 } else { 0x00 });
        bytes.extend_from_slice(&self.value);
        bytes
    }

    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
        if data.is_empty() {
            return Ok(Self::default());
        }
        if data.len() < 1 {
            return Err(StorageValueError::invalid_format(
                "StorageItem requires at least 1 byte for is_constant flag",
            ));
        }
        let is_constant = data[0] != 0;
        let value = data[1..].to_vec();
        Ok(Self { value, is_constant })
    }

    fn storage_size(&self) -> usize {
        1 + self.value.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ StorageKey Tests ============

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
        assert_eq!(key.key().len(), 21); // 1 prefix + 20 bytes hash
        assert_eq!(key.key()[0], 0x14);
    }

    #[test]
    fn test_storage_key_create_with_uint256() {
        let hash = UInt256::zero();
        let key = StorageKey::create_with_uint256(-2, 0x15, &hash);
        assert_eq!(key.id(), -2);
        assert_eq!(key.key().len(), 33); // 1 prefix + 32 bytes hash
        assert_eq!(key.key()[0], 0x15);
    }

    #[test]
    fn test_storage_key_create_with_int32() {
        let key = StorageKey::create_with_int32(-1, 0x20, 0x12345678);
        assert_eq!(key.id(), -1);
        assert_eq!(key.key().len(), 5); // 1 prefix + 4 bytes
        assert_eq!(key.key()[0], 0x20);
        // Big endian: 0x12, 0x34, 0x56, 0x78
        assert_eq!(&key.key()[1..], &[0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn test_storage_key_create_with_int64() {
        let key = StorageKey::create_with_int64(-1, 0x21, 0x123456789ABCDEF0u64 as i64);
        assert_eq!(key.id(), -1);
        assert_eq!(key.key().len(), 9); // 1 prefix + 8 bytes
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
        assert_eq!(prefix.len(), 5); // 4 bytes id + 1 byte prefix
        assert_eq!(&prefix[..4], &(-1i32).to_le_bytes());
        assert_eq!(prefix[4], 0x14);
    }

    #[test]
    fn test_storage_key_ordering() {
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        let key3 = StorageKey::new(0, vec![0x01]);

        assert!(key1 < key2);
        assert!(key1 < key3);
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

        assert!(key1 < key2);
    }

    #[test]
    fn test_storage_key_to_array() {
        let key = StorageKey::new(-1, vec![0xAA, 0xBB]);
        let array = key.to_array();
        // First 4 bytes are the ID in little-endian
        assert_eq!(&array[..4], &(-1i32).to_le_bytes());
        // Remaining bytes are the key
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
    fn test_storage_key_suffix() {
        let key = StorageKey::new(-1, vec![0x01, 0x02]);
        assert_eq!(key.suffix(), key.key());
    }

    #[test]
    fn test_storage_key_length() {
        let key = StorageKey::new(-1, vec![0x01, 0x02, 0x03]);
        // PREFIX_LENGTH (5) + key length (3) = 8
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
        let hash1 = key.get_hash_code();
        let hash2 = key.get_hash_code();
        // Same key should produce same hash
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

    // ============ StorageItem Tests ============

    #[test]
    fn test_storage_item_creation() {
        let item = StorageItem::new(vec![0xAA, 0xBB]);
        assert_eq!(item.value(), &[0xAA, 0xBB]);
        assert!(!item.is_constant());
    }

    #[test]
    fn test_storage_item_constant() {
        let item = StorageItem::constant(vec![0xCC]);
        assert!(item.is_constant());
    }

    #[test]
    fn test_storage_item_get_value() {
        let item = StorageItem::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(item.get_value(), vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_storage_item_set_value() {
        let mut item = StorageItem::new(vec![0x01]);
        item.set_value(vec![0x02, 0x03]);
        assert_eq!(item.value(), &[0x02, 0x03]);
    }

    #[test]
    fn test_storage_item_size() {
        let item = StorageItem::new(vec![0x01, 0x02, 0x03, 0x04]);
        assert_eq!(item.size(), 4);
    }

    #[test]
    fn test_storage_item_default() {
        let item = StorageItem::default();
        let empty: &[u8] = &[];
        assert_eq!(item.value(), empty);
        assert!(!item.is_constant());
        assert_eq!(item.size(), 0);
    }

    #[test]
    fn test_storage_item_from_bytes() {
        let item = StorageItem::from_bytes(vec![0xAA, 0xBB]);
        assert_eq!(item.value(), &[0xAA, 0xBB]);
        assert!(!item.is_constant());
    }

    #[test]
    fn test_storage_item_clone() {
        let item1 = StorageItem::new(vec![0x01, 0x02]);
        let item2 = item1.clone();
        assert_eq!(item1, item2);
    }

    #[test]
    fn test_storage_item_equality() {
        let item1 = StorageItem::new(vec![0x01]);
        let item2 = StorageItem::new(vec![0x01]);
        let item3 = StorageItem::new(vec![0x02]);
        let item4 = StorageItem::constant(vec![0x01]);

        assert_eq!(item1, item2);
        assert_ne!(item1, item3);
        assert_ne!(item1, item4); // Different is_constant flag
    }

    #[test]
    fn test_storage_item_debug() {
        let item = StorageItem::new(vec![0x01]);
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

    // ============ SeekDirection Tests ============

    #[test]
    fn test_seek_direction_default() {
        assert_eq!(SeekDirection::default(), SeekDirection::Forward);
    }

    #[test]
    fn test_seek_direction_variants() {
        assert_ne!(SeekDirection::Forward, SeekDirection::Backward);
    }

    #[test]
    fn test_seek_direction_repr_values() {
        assert_eq!(SeekDirection::Forward as i8, 1);
        assert_eq!(SeekDirection::Backward as i8, -1);
    }

    #[test]
    fn test_seek_direction_clone() {
        let dir1 = SeekDirection::Forward;
        let dir2 = dir1;
        assert_eq!(dir1, dir2);
    }

    // ============ TrackState Tests ============

    #[test]
    fn test_track_state_default() {
        assert_eq!(TrackState::default(), TrackState::None);
    }

    #[test]
    fn test_track_state_variants() {
        let states = vec![
            TrackState::None,
            TrackState::Added,
            TrackState::Changed,
            TrackState::Deleted,
            TrackState::NotFound,
        ];

        for (i, state1) in states.iter().enumerate() {
            for (j, state2) in states.iter().enumerate() {
                if i == j {
                    assert_eq!(state1, state2);
                } else {
                    assert_ne!(state1, state2);
                }
            }
        }
    }

    #[test]
    fn test_track_state_repr_values() {
        assert_eq!(TrackState::None as u8, 0);
        assert_eq!(TrackState::Added as u8, 1);
        assert_eq!(TrackState::Changed as u8, 2);
        assert_eq!(TrackState::Deleted as u8, 3);
        assert_eq!(TrackState::NotFound as u8, 4);
    }

    #[test]
    fn test_track_state_clone() {
        let state1 = TrackState::Changed;
        let state2 = state1;
        assert_eq!(state1, state2);
    }

    // ============ Serde Tests ============

    #[test]
    fn test_serde_storage_key() {
        let key = StorageKey::new(-1, vec![0x01, 0x02]);
        let serialized = serde_json::to_string(&key).unwrap();
        let deserialized: StorageKey = serde_json::from_str(&serialized).unwrap();
        assert_eq!(key.id, deserialized.id);
        assert_eq!(key.key, deserialized.key);
    }

    #[test]
    fn test_serde_storage_item() {
        let item = StorageItem::constant(vec![0xAA, 0xBB]);
        let serialized = serde_json::to_string(&item).unwrap();
        let deserialized: StorageItem = serde_json::from_str(&serialized).unwrap();
        assert_eq!(item, deserialized);
        assert!(deserialized.is_constant());
    }

    #[test]
    fn test_serde_seek_direction() {
        let dir = SeekDirection::Backward;
        let serialized = serde_json::to_string(&dir).unwrap();
        let deserialized: SeekDirection = serde_json::from_str(&serialized).unwrap();
        assert_eq!(dir, deserialized);
    }

    #[test]
    fn test_serde_track_state() {
        let state = TrackState::Changed;
        let serialized = serde_json::to_string(&state).unwrap();
        let deserialized: TrackState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(state, deserialized);
    }

    // ============ IStorageValue Tests ============

    #[test]
    fn test_storage_item_to_storage_bytes() {
        let item = StorageItem::new(vec![0xAA, 0xBB, 0xCC]);
        let bytes = item.to_storage_bytes();
        // First byte is is_constant flag (0x00 for false)
        assert_eq!(bytes[0], 0x00);
        // Remaining bytes are the value
        assert_eq!(&bytes[1..], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_storage_item_to_storage_bytes_constant() {
        let item = StorageItem::constant(vec![0x01, 0x02]);
        let bytes = item.to_storage_bytes();
        // First byte is is_constant flag (0x01 for true)
        assert_eq!(bytes[0], 0x01);
        assert_eq!(&bytes[1..], &[0x01, 0x02]);
    }

    #[test]
    fn test_storage_item_from_storage_bytes() {
        let data = vec![0x00, 0xAA, 0xBB];
        let item = StorageItem::from_storage_bytes(&data).unwrap();
        assert!(!item.is_constant());
        assert_eq!(item.value(), &[0xAA, 0xBB]);
    }

    #[test]
    fn test_storage_item_from_storage_bytes_constant() {
        let data = vec![0x01, 0x11, 0x22, 0x33];
        let item = StorageItem::from_storage_bytes(&data).unwrap();
        assert!(item.is_constant());
        assert_eq!(item.value(), &[0x11, 0x22, 0x33]);
    }

    #[test]
    fn test_storage_item_from_storage_bytes_empty() {
        let data: &[u8] = &[];
        let item = StorageItem::from_storage_bytes(data).unwrap();
        assert!(!item.is_constant());
        let empty: &[u8] = &[];
        assert_eq!(item.value(), empty);
    }

    #[test]
    fn test_storage_item_from_storage_bytes_only_flag() {
        let data = vec![0x01]; // Only is_constant flag, no value
        let item = StorageItem::from_storage_bytes(&data).unwrap();
        assert!(item.is_constant());
        let empty: &[u8] = &[];
        assert_eq!(item.value(), empty);
    }

    #[test]
    fn test_storage_item_storage_size() {
        let item = StorageItem::new(vec![0x01, 0x02, 0x03]);
        // 1 byte for is_constant + 3 bytes for value = 4
        assert_eq!(item.storage_size(), 4);
    }

    #[test]
    fn test_storage_item_storage_size_empty() {
        let item = StorageItem::new(vec![]);
        // 1 byte for is_constant + 0 bytes for value = 1
        assert_eq!(item.storage_size(), 1);
    }

    #[test]
    fn test_storage_item_roundtrip() {
        let original = StorageItem::new(vec![0x00, 0xFF, 0x12, 0x34]);
        let bytes = original.to_storage_bytes();
        let restored = StorageItem::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original.value(), restored.value());
        assert_eq!(original.is_constant(), restored.is_constant());
    }

    #[test]
    fn test_storage_item_roundtrip_constant() {
        let original = StorageItem::constant(vec![0xDE, 0xAD, 0xBE, 0xEF]);
        let bytes = original.to_storage_bytes();
        let restored = StorageItem::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original.value(), restored.value());
        assert_eq!(original.is_constant(), restored.is_constant());
    }

    #[test]
    fn test_storage_item_roundtrip_large() {
        let large_data: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let original = StorageItem::new(large_data.clone());
        let bytes = original.to_storage_bytes();
        let restored = StorageItem::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original.value(), restored.value());
        assert_eq!(original.storage_size(), 1001); // 1 + 1000
    }

    #[test]
    fn test_storage_item_istorage_value_trait_object() {
        // Test that StorageItem can be used as a trait object
        fn use_storage_value<V: IStorageValue>(value: &V) -> usize {
            value.storage_size()
        }

        let item = StorageItem::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(use_storage_value(&item), 4);
    }
}

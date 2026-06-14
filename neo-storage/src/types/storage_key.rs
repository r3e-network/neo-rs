use crate::hash_utils::XxHash3;
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Storage key for Neo blockchain.
///
/// Represents the keys in contract storage, matching C# Neo.SmartContract.StorageKey exactly.
/// Combines a contract ID with a key suffix to form a unique storage key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageKey {
    /// Contract ID (native contracts use negative IDs).
    pub id: i32,
    /// Key suffix (variable length).
    key: Vec<u8>,
    /// Cached full key (optional).
    #[serde(skip)]
    cache: Option<Vec<u8>>,
}

impl PartialEq for StorageKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.key == other.key
    }
}

impl Eq for StorageKey {}

impl std::hash::Hash for StorageKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.key.hash(state);
    }
}

impl StorageKey {
    /// Prefix length: sizeof(i32) + sizeof(u8) = 5 bytes.
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Creates a new storage key.
    #[must_use]
    pub const fn new(id: i32, key: Vec<u8>) -> Self {
        Self {
            id,
            key,
            cache: None,
        }
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
    #[must_use]
    pub fn length(&self) -> usize {
        if let Some(ref cache) = self.cache {
            cache.len()
        } else {
            Self::PREFIX_LENGTH + self.key.len()
        }
    }

    /// Creates a storage key with a single-byte prefix.
    #[must_use]
    pub fn create(id: i32, prefix: u8) -> Self {
        let key = Self::storage_key(prefix, &[]);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and single byte content.
    #[must_use]
    pub fn create_with_byte(id: i32, prefix: u8, content: u8) -> Self {
        let key = Self::storage_key(prefix, &[content]);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and `UInt160` hash.
    #[must_use]
    pub fn create_with_uint160(id: i32, prefix: u8, hash: &UInt160) -> Self {
        let key = Self::storage_key(prefix, &hash.as_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and `UInt256` hash.
    #[must_use]
    pub fn create_with_uint256(id: i32, prefix: u8, hash: &UInt256) -> Self {
        let key = Self::storage_key(prefix, &hash.as_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix, `UInt256` hash, and `UInt160` signer.
    #[must_use]
    pub fn create_with_uint256_uint160(
        id: i32,
        prefix: u8,
        hash: &UInt256,
        signer: &UInt160,
    ) -> Self {
        let mut suffix = Vec::with_capacity(32 + 20);
        suffix.extend_from_slice(&hash.as_bytes());
        suffix.extend_from_slice(&signer.as_bytes());
        let key = Self::storage_key(prefix, &suffix);
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and i32 value (big endian).
    #[must_use]
    pub fn create_with_int32(id: i32, prefix: u8, big_endian: i32) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and u32 value (big endian).
    #[must_use]
    pub fn create_with_uint32(id: i32, prefix: u8, big_endian: u32) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and i64 value (big endian).
    #[must_use]
    pub fn create_with_int64(id: i32, prefix: u8, big_endian: i64) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and u64 value (big endian).
    #[must_use]
    pub fn create_with_uint64(id: i32, prefix: u8, big_endian: u64) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Creates a storage key with prefix and byte content.
    #[must_use]
    pub fn create_with_bytes(id: i32, prefix: u8, content: &[u8]) -> Self {
        let key = Self::storage_key(prefix, content);
        Self::new(id, key)
    }

    /// Creates a search prefix for iterating contract storage.
    #[must_use]
    pub fn create_search_prefix(id: i32, prefix: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + prefix.len()];
        buffer[..4].copy_from_slice(&id.to_le_bytes());
        buffer[4..].copy_from_slice(prefix);
        buffer
    }

    /// Returns the contract ID.
    #[must_use]
    pub const fn id(&self) -> i32 {
        self.id
    }

    /// Returns the key suffix.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Alias for `key()` - returns the suffix portion (excluding contract ID).
    #[must_use]
    pub fn suffix(&self) -> &[u8] {
        &self.key
    }

    /// Returns the full key bytes as a slice (zero-copy when cache is populated).
    ///
    /// Callers that only need to read the bytes should prefer this over `to_array()`
    /// to avoid an allocation.
    #[must_use]
    pub fn as_bytes(&self) -> std::borrow::Cow<'_, [u8]> {
        if let Some(ref cache) = self.cache {
            std::borrow::Cow::Borrowed(cache)
        } else {
            std::borrow::Cow::Owned(self.build())
        }
    }

    /// Converts the storage key to an owned byte array for storage.
    #[must_use]
    pub fn to_array(&self) -> Vec<u8> {
        if let Some(ref cache) = self.cache {
            cache.clone()
        } else {
            self.build()
        }
    }

    /// Returns the hash code using the same algorithm as the C# implementation.
    #[must_use]
    pub fn hash_code(&self) -> i32 {
        let seed = XxHash3::default_xx_hash3_seed();
        let suffix_hash = XxHash3::xx_hash3_32(&self.key, seed);
        XxHash3::hash_code_combine_i32(self.id, suffix_hash)
    }

    /// Builds the full key bytes.
    fn build(&self) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + self.key.len()];
        buffer[..4].copy_from_slice(&self.id.to_le_bytes());
        buffer[4..].copy_from_slice(&self.key);
        buffer
    }

    /// Creates a storage key from raw bytes.
    #[must_use]
    pub fn from_bytes(cache: &[u8]) -> Self {
        if cache.len() < 4 {
            return Self {
                id: 0,
                key: cache.to_vec(),
                cache: Some(cache.to_vec()),
            };
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
                    .map(|b| format!("0x{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
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
}

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
        let left = self.as_bytes();
        let right = other.as_bytes();
        left.as_ref().cmp(right.as_ref())
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
mod tests;

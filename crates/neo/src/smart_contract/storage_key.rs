//! StorageKey - matches C# Neo.SmartContract.StorageKey exactly

use crate::{UInt160, UInt256};
use std::fmt;

/// Represents the keys in contract storage (matches C# StorageKey)
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorageKey {
    /// The id of the contract
    pub id: i32,

    /// The key of the storage entry
    key: Vec<u8>,

    /// Cached full key
    cache: Option<Vec<u8>>,
}

impl StorageKey {
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Creates a new StorageKey
    pub fn new(id: i32, key: Vec<u8>) -> Self {
        Self {
            id,
            key,
            cache: None,
        }
    }

    #[inline]
    fn storage_key(prefix: u8, suffix: &[u8]) -> Vec<u8> {
        let mut key = Vec::with_capacity(1 + suffix.len());
        key.push(prefix);
        key.extend_from_slice(suffix);
        key
    }

    /// Get key length
    pub fn length(&self) -> usize {
        if self.cache.is_none() {
            return Self::PREFIX_LENGTH + self.key.len();
        }
        self.cache.as_ref().unwrap().len()
    }

    /// Create StorageKey with just prefix
    pub fn create(id: i32, prefix: u8) -> Self {
        let key = Self::storage_key(prefix, &[]);
        Self::new(id, key)
    }

    /// Create StorageKey with byte content
    pub fn create_with_byte(id: i32, prefix: u8, content: u8) -> Self {
        let key = Self::storage_key(prefix, &[content]);
        Self::new(id, key)
    }

    /// Create StorageKey with UInt160
    pub fn create_with_uint160(id: i32, prefix: u8, hash: &UInt160) -> Self {
        let key = Self::storage_key(prefix, hash.to_bytes().as_ref());
        Self::new(id, key)
    }

    /// Create StorageKey with UInt256
    pub fn create_with_uint256(id: i32, prefix: u8, hash: &UInt256) -> Self {
        let key = Self::storage_key(prefix, hash.to_bytes().as_ref());
        Self::new(id, key)
    }

    /// Create StorageKey with UInt256 and UInt160
    pub fn create_with_uint256_uint160(
        id: i32,
        prefix: u8,
        hash: &UInt256,
        signer: &UInt160,
    ) -> Self {
        let mut suffix = hash.to_bytes();
        suffix.extend_from_slice(&signer.to_bytes());
        let key = Self::storage_key(prefix, &suffix);
        Self::new(id, key)
    }

    /// Create StorageKey with int32 (big endian)
    pub fn create_with_int32(id: i32, prefix: u8, big_endian: i32) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Create StorageKey with int64 (big endian)
    pub fn create_with_int64(id: i32, prefix: u8, big_endian: i64) -> Self {
        let key = Self::storage_key(prefix, &big_endian.to_be_bytes());
        Self::new(id, key)
    }

    /// Create StorageKey with bytes
    pub fn create_with_bytes(id: i32, prefix: u8, content: &[u8]) -> Self {
        let key = Self::storage_key(prefix, content);
        Self::new(id, key)
    }

    /// Creates a search prefix for a contract
    pub fn create_search_prefix(id: i32, prefix: &[u8]) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + prefix.len()];
        buffer[..4].copy_from_slice(&id.to_le_bytes());
        buffer[4..].copy_from_slice(prefix);
        buffer
    }

    /// Returns the raw key bytes (excluding the contract ID prefix).
    pub fn suffix(&self) -> &[u8] {
        &self.key
    }

    /// Convert to byte array
    pub fn to_array(&self) -> Vec<u8> {
        if self.cache.is_none() {
            return self.build();
        }
        self.cache.as_ref().unwrap().clone()
    }

    fn build(&self) -> Vec<u8> {
        let mut buffer = vec![0u8; std::mem::size_of::<i32>() + self.key.len()];
        buffer[..4].copy_from_slice(&self.id.to_le_bytes());
        buffer[4..].copy_from_slice(&self.key);
        buffer
    }

    /// Create from bytes
    pub fn from_bytes(cache: &[u8]) -> Self {
        let id = i32::from_le_bytes([cache[0], cache[1], cache[2], cache[3]]);
        let key = cache[4..].to_vec();
        Self {
            id,
            key,
            cache: Some(cache.to_vec()),
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

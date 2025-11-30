//! Storage types for Neo blockchain.

use serde::{Deserialize, Serialize};

/// Direction for seeking in storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SeekDirection {
    /// Seek forward (ascending order).
    Forward,
    /// Seek backward (descending order).
    Backward,
}

impl Default for SeekDirection {
    fn default() -> Self {
        Self::Forward
    }
}

/// Track state for cached storage items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrackState {
    /// Item has not been modified.
    None,
    /// Item has been added.
    Added,
    /// Item has been modified.
    Changed,
    /// Item has been deleted.
    Deleted,
}

impl Default for TrackState {
    fn default() -> Self {
        Self::None
    }
}

/// Storage key for Neo blockchain.
///
/// Combines a contract ID with a key suffix to form a unique storage key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    /// Contract ID (native contracts use negative IDs).
    id: i32,
    /// Key suffix (variable length).
    key: Vec<u8>,
}

impl StorageKey {
    /// Creates a new storage key.
    pub fn new(id: i32, key: Vec<u8>) -> Self {
        Self { id, key }
    }

    /// Creates a storage key with a single-byte prefix.
    pub fn create(id: i32, prefix: u8) -> Self {
        Self {
            id,
            key: vec![prefix],
        }
    }

    /// Creates a storage key from raw bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            id: 0,
            key: bytes.to_vec(),
        }
    }

    /// Returns the contract ID.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Returns the key suffix.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Alias for key() - returns the suffix portion.
    pub fn suffix(&self) -> &[u8] {
        &self.key
    }

    /// Converts the storage key to a byte array for storage.
    pub fn to_array(&self) -> Vec<u8> {
        let mut result = Vec::with_capacity(4 + self.key.len());
        result.extend_from_slice(&self.id.to_le_bytes());
        result.extend_from_slice(&self.key);
        result
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

    /// Returns the stored value.
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
    fn test_storage_key_ordering() {
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        let key3 = StorageKey::new(0, vec![0x01]);

        assert!(key1 < key2);
        assert!(key1 < key3);
    }

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
    fn test_seek_direction_default() {
        assert_eq!(SeekDirection::default(), SeekDirection::Forward);
    }

    #[test]
    fn test_track_state_default() {
        assert_eq!(TrackState::default(), TrackState::None);
    }
}

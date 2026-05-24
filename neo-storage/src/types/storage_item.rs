use neo_primitives::{IStorageValue, StorageValueResult};
use serde::{Deserialize, Serialize};

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
    #[must_use]
    pub const fn new(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: false,
        }
    }

    /// Creates a storage item from bytes.
    #[must_use]
    pub fn from_bytes(value: Vec<u8>) -> Self {
        Self::new(value)
    }

    /// Creates a constant storage item.
    #[must_use]
    pub const fn constant(value: Vec<u8>) -> Self {
        Self {
            value,
            is_constant: true,
        }
    }

    /// Returns a clone of the stored value.
    #[must_use]
    pub fn get_value(&self) -> Vec<u8> {
        self.value.clone()
    }

    /// Returns a reference to the stored value.
    #[must_use]
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Sets the stored value.
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = value;
    }

    /// Returns whether this item is constant.
    #[must_use]
    pub const fn is_constant(&self) -> bool {
        self.is_constant
    }

    /// Returns the size of the stored value.
    #[must_use]
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
        bytes.push(u8::from(self.is_constant));
        bytes.extend_from_slice(&self.value);
        bytes
    }

    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
        if data.is_empty() {
            return Ok(Self::default());
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

    #[test]
    fn test_serde_storage_item() {
        let item = StorageItem::constant(vec![0xAA, 0xBB]);
        let serialized = serde_json::to_string(&item).unwrap();
        let deserialized: StorageItem = serde_json::from_str(&serialized).unwrap();
        assert_eq!(item, deserialized);
        assert!(deserialized.is_constant());
    }

    #[test]
    fn test_storage_item_to_storage_bytes() {
        let item = StorageItem::new(vec![0xAA, 0xBB, 0xCC]);
        let bytes = item.to_storage_bytes();
        assert_eq!(bytes[0], 0x00);
        assert_eq!(&bytes[1..], &[0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_storage_item_to_storage_bytes_constant() {
        let item = StorageItem::constant(vec![0x01, 0x02]);
        let bytes = item.to_storage_bytes();
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
        let data = vec![0x01];
        let item = StorageItem::from_storage_bytes(&data).unwrap();
        assert!(item.is_constant());
        let empty: &[u8] = &[];
        assert_eq!(item.value(), empty);
    }

    #[test]
    fn test_storage_item_storage_size() {
        let item = StorageItem::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(item.storage_size(), 4);
    }

    #[test]
    fn test_storage_item_storage_size_empty() {
        let item = StorageItem::new(vec![]);
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
        assert_eq!(original.storage_size(), 1001);
    }

    #[test]
    fn test_storage_item_istorage_value_trait_object() {
        fn use_storage_value<V: IStorageValue>(value: &V) -> usize {
            value.storage_size()
        }

        let item = StorageItem::new(vec![0x01, 0x02, 0x03]);
        assert_eq!(use_storage_value(&item), 4);
    }
}

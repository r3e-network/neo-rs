use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use neo_primitives::{StorageValue, StorageValueResult};
use serde::{Deserialize, Serialize};

/// Trait for opaque cache values stored in `StorageItem`.
///
/// This allows higher-level crates (e.g. neo-core) to store type-specific
/// caches (BigInteger, Interoperable) without neo-storage depending on
/// those types directly.
pub trait CacheProvider: Send + Sync + fmt::Debug {
    /// Serialize the cached value to bytes.
    fn to_bytes(&self) -> Vec<u8>;
    /// Clone into a new boxed trait object.
    fn clone_box(&self) -> Box<dyn CacheProvider>;
    /// Upcast to `&dyn Any` so callers can `downcast_ref` to a concrete type.
    fn as_any(&self) -> &dyn Any;
}

/// Storage item for Neo blockchain.
///
/// Represents a value stored in contract storage, optionally backed by a typed
/// cache (BigInteger / Interoperable) for lazy materialisation.
///
/// Mirrors C# `Neo.SmartContract.StorageItem` (v3.9): a plain value with no
/// `IsConstant` flag. Its only serialized form is the raw value bytes
/// (`StorageItem.Serialize => writer.Write(Value.Span)`).
#[derive(Serialize, Deserialize)]
pub struct StorageItem {
    /// The stored value bytes.
    value: Vec<u8>,
    /// Optional typed cache for lazy materialisation.
    #[serde(skip)]
    cache: Option<Box<dyn CacheProvider>>,
}

impl StorageItem {
    /// Creates a new empty storage item.
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: Vec::new(),
            cache: None,
        }
    }

    /// Creates a storage item from bytes.
    #[must_use]
    pub fn from_bytes(value: Vec<u8>) -> Self {
        Self { value, cache: None }
    }

    /// Returns a clone of the stored value, converting from cache when the
    /// raw bytes are empty but a cache is present.
    #[must_use]
    pub fn to_value(&self) -> Vec<u8> {
        if !self.value.is_empty() || self.cache.is_none() {
            return self.value.clone();
        }
        self.cache.as_ref().unwrap().to_bytes()
    }

    /// Returns a reference to the raw stored bytes (may be empty even when a
    /// cache is populated).
    #[must_use]
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Returns the value bytes, borrowing when possible and falling back to
    /// cache materialisation when the raw bytes are empty.
    #[must_use]
    pub fn value_bytes(&self) -> Cow<'_, [u8]> {
        if !self.value.is_empty() || self.cache.is_none() {
            Cow::Borrowed(&self.value)
        } else {
            Cow::Owned(self.cache.as_ref().unwrap().to_bytes())
        }
    }

    /// Sets the stored value and clears the cache.
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = value;
        self.cache = None;
    }

    /// Returns the size of the stored value.
    #[must_use]
    pub fn size(&self) -> usize {
        self.value.len()
    }

    /// Sets the opaque cache value.
    pub fn set_cache(&mut self, cache: Box<dyn CacheProvider>) {
        self.cache = Some(cache);
    }

    /// Returns a reference to the cache, if present.
    #[must_use]
    pub fn cache(&self) -> Option<&dyn CacheProvider> {
        self.cache.as_deref()
    }

    /// Returns `true` when a cache is present.
    #[must_use]
    pub fn has_cache(&self) -> bool {
        self.cache.is_some()
    }

    /// Clears the cache without affecting the stored bytes.
    pub fn clear_cache(&mut self) {
        self.cache = None;
    }

    /// Clears the stored bytes without affecting the cache.
    ///
    /// This is used by higher-level crates (e.g. neo-core) when setting a
    /// cache-backed value that should materialise lazily.
    pub fn clear_value(&mut self) {
        self.value.clear();
    }

    /// Materialises the cache into the value bytes (no-op when bytes are
    /// already populated or no cache is present).
    pub fn seal(&mut self) {
        if self.value.is_empty() {
            self.value = self.to_value();
        }
    }

    /// Copies value and cache from another instance.
    pub fn from_replica(&mut self, replica: &StorageItem) {
        self.value = replica.value.clone();
        self.cache = replica.cache.as_ref().map(|c| c.clone_box());
    }

    /// Convenience helper to populate the value from raw bytes.
    pub fn deserialize_from_bytes(&mut self, data: &[u8]) {
        self.value = data.to_vec();
        self.cache = None;
    }
}

// Manual impls needed because `dyn CacheProvider` does not satisfy
// the derive requirements for Clone / PartialEq / Debug / Default.

impl Clone for StorageItem {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            cache: self.cache.as_ref().map(|c| c.clone_box()),
        }
    }
}

impl PartialEq for StorageItem {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for StorageItem {}

neo_io::impl_default_via_new!(StorageItem);

impl fmt::Debug for StorageItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageItem")
            .field("value", &self.value)
            .field("has_cache", &self.cache.is_some())
            .finish()
    }
}

impl fmt::Display for StorageItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value_array = self.to_value();
        write!(
            f,
            "Value = {{ {} }}",
            value_array
                .iter()
                .map(|b| format!("0x{:02x}", b))
                .collect::<Vec<_>>()
                .join(", ")
        )
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

/// Implement `StorageValue` trait from neo-primitives.
///
/// This allows `StorageItem` to be used with generic storage abstractions
/// that require the `StorageValue` trait, breaking the circular dependency
/// between neo-storage and neo-vm.
///
/// # Serialization Format
///
/// The storage format is the raw value bytes, matching C# v3.9
/// `StorageItem.Serialize => writer.Write(Value.Span)` (no flags or prefixes),
/// so persisted bytes / state roots stay byte-identical to C#.
impl StorageValue for StorageItem {
    fn to_storage_bytes(&self) -> Vec<u8> {
        self.to_value()
    }

    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
        Ok(Self::from_bytes(data.to_vec()))
    }

    fn storage_size(&self) -> usize {
        // Must match to_storage_bytes(): to_value() materializes the cache, so a
        // cache-only item (empty `value`) still reports its true serialized size.
        self.to_value().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(item.size(), 4);
    }

    #[test]
    fn test_storage_item_default() {
        let item = StorageItem::default();
        let empty: &[u8] = &[];
        assert_eq!(item.value(), empty);
        assert_eq!(item.size(), 0);
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
}

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use neo_io::serializable::helper::SerializeHelper;
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
/// Mirrors C# `Neo.SmartContract.StorageItem` (v3.10): a plain value with no
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
        match &self.cache {
            Some(cache) if self.value.is_empty() => Cow::Owned(cache.to_bytes()),
            _ => Cow::Borrowed(&self.value),
        }
    }

    /// Sets the stored value and clears the cache.
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = value;
        self.cache = None;
    }

    /// Returns the C# `StorageItem.Size`: `Value.GetVarSize()`.
    #[must_use]
    pub fn size(&self) -> usize {
        SerializeHelper::get_var_size_bytes(&self.value_bytes())
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
        self.value_bytes() == other.value_bytes()
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
/// The storage format is the raw value bytes, matching C# v3.10
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
#[path = "../tests/types/storage_item.rs"]
mod tests;

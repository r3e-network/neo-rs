//! Cache-aware `StorageItem` extension.
//!
//! `StorageItem` itself is canonical in `neo-storage` (a leaf storage crate).
//! This module adds the cache-aware BigInteger extension trait. Keeping it here
//! — rather than in `persistence` — means the storage/persistence layer carries
//! no edge back into the smart-contract layer.

use neo_io::serializable::helper::SerializeHelper;
use neo_io::{IoResult, MemoryReader};
use neo_storage::{StorageItem, StorageItemCache};
use num_bigint::BigInt;

// ---------------------------------------------------------------------------
// StorageItemExt – extension trait for cache-aware operations
// ---------------------------------------------------------------------------

/// Extension methods on [`StorageItem`] that require smart-contract types.
///
/// Import this trait wherever you need `to_bigint`, `set_bigint`, `add_ext`,
/// `from_bigint`, `serialized_size`, or `deserialize_reader`.
pub trait StorageItemExt {
    /// Initializes with a BigInteger cache.
    fn from_bigint(value: BigInt) -> Self;

    /// Increases the integer value by the specified amount.
    fn add_ext(&mut self, integer: BigInt);

    /// Sets the integer value (clears the current bytes).
    fn set_bigint(&mut self, integer: BigInt);

    /// Converts the stored value to a `BigInt`.
    fn to_bigint(&self) -> BigInt;

    /// Returns the encoded size (var-size prefix + payload).
    fn serialized_size(&self) -> usize;

    /// Deserialize from a memory reader (matches the C# signature).
    fn deserialize_reader(&mut self, reader: &mut MemoryReader<'_>) -> IoResult<()>;
}

impl StorageItemExt for StorageItem {
    fn from_bigint(value: BigInt) -> Self {
        let mut item = Self::new();
        item.set_cache(StorageItemCache::big_integer(value));
        item
    }

    fn add_ext(&mut self, integer: BigInt) {
        let current = self.to_bigint();
        self.set_bigint(current + integer);
    }

    fn set_bigint(&mut self, integer: BigInt) {
        self.set_cache(StorageItemCache::big_integer(integer));
        self.clear_value();
    }

    fn to_bigint(&self) -> BigInt {
        if let Some(cache) = self.cache() {
            if let Some(value) = cache.as_big_integer() {
                return value.clone();
            }
        }
        let bytes = self.value_bytes();
        if bytes.is_empty() {
            return BigInt::ZERO;
        }
        BigInt::from_signed_bytes_le(&bytes)
    }

    fn serialized_size(&self) -> usize {
        SerializeHelper::get_var_size_bytes(&self.value_bytes())
    }

    fn deserialize_reader(&mut self, reader: &mut MemoryReader<'_>) -> IoResult<()> {
        let data = reader.read_to_end()?.to_vec();
        self.deserialize_from_bytes(&data);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/storage/storage_item_ext.rs"]
mod tests;

//! Cache-aware `StorageItem` extension.
//!
//! `StorageItem` itself is canonical in `neo-storage` (a leaf storage crate).
//! This module adds the cache-aware BigInteger extension trait. Keeping it here
//! — rather than in `persistence` — means the storage/persistence layer carries
//! no edge back into the smart-contract layer.

use neo_io::serializable::helper::SerializeHelper;
use neo_io::{IoResult, MemoryReader};
use neo_storage::StorageItem;
use neo_storage::types::storage_item::CacheProvider;
use num_bigint::BigInt;
use std::any::Any;

// ---------------------------------------------------------------------------
// StorageCache – internal enum for BigInteger payloads
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug)]
enum StorageCache {
    BigInteger(BigInt),
}

impl Clone for StorageCache {
    fn clone(&self) -> Self {
        match self {
            StorageCache::BigInteger(value) => StorageCache::BigInteger(value.clone()),
        }
    }
}

impl CacheProvider for StorageCache {
    fn to_bytes(&self) -> Vec<u8> {
        match self {
            StorageCache::BigInteger(value) => {
                // C# `BigInteger.ToByteArrayStandard()` stores zero as an
                // empty byte array, not `[0x00]`. These raw bytes feed state
                // root calculation when cache-backed items are sealed.
                if value == &BigInt::ZERO {
                    Vec::new()
                } else {
                    value.to_signed_bytes_le()
                }
            }
        }
    }

    fn clone_box(&self) -> Box<dyn CacheProvider> {
        Box::new(self.clone())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

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
        item.set_cache(Box::new(StorageCache::BigInteger(value)));
        item
    }

    fn add_ext(&mut self, integer: BigInt) {
        let current = self.to_bigint();
        self.set_bigint(current + integer);
    }

    fn set_bigint(&mut self, integer: BigInt) {
        self.set_cache(Box::new(StorageCache::BigInteger(integer)));
        self.clear_value();
    }

    fn to_bigint(&self) -> BigInt {
        // Try to recover a cached BigInteger.
        if let Some(cache) = self.cache() {
            if let Some(StorageCache::BigInteger(v)) = cache.as_any().downcast_ref::<StorageCache>()
            {
                return v.clone();
            }
        }
        // Fallback: decode from bytes.
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
mod tests {
    use super::*;

    #[test]
    fn cache_backed_zero_bigint_materializes_as_empty_bytes() {
        let item = StorageItem::from_bigint(BigInt::from(0));

        assert!(item.value_bytes().is_empty());
        assert_eq!(item.to_value(), Vec::<u8>::new());
        assert_eq!(item.serialized_size(), 1);
    }

    #[test]
    fn set_bigint_zero_clears_previous_bytes() {
        let mut item = StorageItem::from_bytes(vec![0x01]);

        item.set_bigint(BigInt::from(0));

        assert!(item.value().is_empty());
        assert!(item.value_bytes().is_empty());
        assert_eq!(item.to_bigint(), BigInt::from(0));
    }

    #[test]
    fn nonzero_bigint_materializes_as_signed_little_endian() {
        let value = BigInt::from(128);
        let item = StorageItem::from_bigint(value.clone());

        assert_eq!(item.value_bytes().as_ref(), value.to_signed_bytes_le());
    }
}

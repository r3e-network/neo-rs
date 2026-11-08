//! StorageItem - matches C# Neo.SmartContract.StorageItem.
//!
//! The canonical definition lives in `neo-storage`. This module provides the
//! contract-owned cache extension for BigInteger and Interoperable payloads.

use neo_io::serializable::helper::get_var_size_bytes;
use neo_io::{IoResult, MemoryReader};
use neo_storage::types::storage_item::CacheProvider;
use neo_vm::{BinarySerializer, ExecutionEngineLimits, Interoperable};
use num_bigint::BigInt;
use std::any::Any;

// Re-export the canonical StorageItem from neo-storage.
pub use neo_storage::StorageItem;

// ---------------------------------------------------------------------------
// StorageCache – internal enum for BigInteger / Interoperable payloads
// ---------------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug)]
enum StorageCache {
    BigInteger(BigInt),
    Interoperable(Box<dyn Interoperable>),
}

impl Clone for StorageCache {
    fn clone(&self) -> Self {
        match self {
            StorageCache::BigInteger(value) => StorageCache::BigInteger(value.clone()),
            StorageCache::Interoperable(value) => StorageCache::Interoperable(value.clone_box()),
        }
    }
}

impl CacheProvider for StorageCache {
    fn to_bytes(&self) -> Vec<u8> {
        match self {
            StorageCache::BigInteger(value) => {
                let mut bytes = value.to_signed_bytes_le();
                if bytes.is_empty() {
                    bytes.push(0);
                }
                bytes
            }
            StorageCache::Interoperable(interoperable) => match interoperable.to_stack_item() {
                Ok(item) => BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
                    .unwrap_or_default(),
                Err(_) => Vec::new(),
            },
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

/// Extension methods on [`StorageItem`] for contract execution caches.
///
/// Import this trait wherever you need `to_bigint`, `set_bigint`, `add`,
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
        if let Some(cache) = self.cache()
            && let Some(sc) = cache.as_any().downcast_ref::<StorageCache>()
            && let StorageCache::BigInteger(v) = sc
        {
            return v.clone();
        }
        // Fallback: decode from bytes.
        let bytes = self.value_bytes();
        if bytes.is_empty() {
            return BigInt::ZERO;
        }
        BigInt::from_signed_bytes_le(&bytes)
    }

    fn serialized_size(&self) -> usize {
        get_var_size_bytes(&self.value_bytes())
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
    use num_bigint::BigInt;

    #[test]
    fn from_bigint_creates_item_with_cached_value() {
        let value = BigInt::from(42);
        let item = StorageItem::from_bigint(value.clone());
        assert_eq!(item.to_bigint(), value);
    }

    #[test]
    fn to_bigint_returns_zero_for_empty_item() {
        let item = StorageItem::new();
        assert_eq!(item.to_bigint(), BigInt::ZERO);
    }

    #[test]
    fn to_bigint_decodes_from_bytes_when_no_cache() {
        let mut item = StorageItem::new();
        let value = BigInt::from(12345);
        let bytes = value.to_signed_bytes_le();
        item.deserialize_from_bytes(&bytes);
        assert_eq!(item.to_bigint(), value);
    }

    #[test]
    fn set_bigint_clears_value_and_sets_cache() {
        let mut item = StorageItem::new();
        item.deserialize_from_bytes(&[1, 2, 3]);

        let new_value = BigInt::from(999);
        item.set_bigint(new_value.clone());

        assert_eq!(item.to_bigint(), new_value);
        assert!(item.value_bytes().is_empty());
    }

    #[test]
    fn add_ext_increments_bigint_value() {
        let mut item = StorageItem::from_bigint(BigInt::from(10));
        item.add_ext(BigInt::from(5));
        assert_eq!(item.to_bigint(), BigInt::from(15));
    }

    #[test]
    fn add_ext_works_with_negative_numbers() {
        let mut item = StorageItem::from_bigint(BigInt::from(10));
        item.add_ext(BigInt::from(-3));
        assert_eq!(item.to_bigint(), BigInt::from(7));
    }

    #[test]
    fn serialized_size_returns_correct_var_size() {
        let mut item = StorageItem::new();
        item.deserialize_from_bytes(&[1, 2, 3]);
        // var-size for 3 bytes is 1 byte prefix + 3 bytes = 4
        assert_eq!(item.serialized_size(), 4);
    }

    #[test]
    fn serialized_size_for_empty_item() {
        let item = StorageItem::new();
        // var-size for 0 bytes is 1 byte (0x00)
        assert_eq!(item.serialized_size(), 1);
    }

    #[test]
    fn cache_to_bytes_returns_bigint_bytes() {
        let value = BigInt::from(256);
        let cache = StorageCache::BigInteger(value.clone());
        let bytes = cache.to_bytes();
        assert_eq!(BigInt::from_signed_bytes_le(&bytes), value);
    }

    #[test]
    fn cache_to_bytes_returns_zero_byte_for_zero() {
        let value = BigInt::ZERO;
        let cache = StorageCache::BigInteger(value);
        let bytes = cache.to_bytes();
        assert_eq!(bytes, vec![0]);
    }

    #[test]
    fn storage_cache_clone_preserves_bigint() {
        let cache = StorageCache::BigInteger(BigInt::from(42));
        let cloned = cache.clone();

        if let StorageCache::BigInteger(val) = cloned {
            assert_eq!(val, BigInt::from(42));
        } else {
            panic!("Expected BigInteger variant");
        }
    }

    #[test]
    fn deserialize_reader_loads_bytes_into_item() {
        let data = vec![1, 2, 3, 4, 5];
        let mut reader = MemoryReader::new(&data);
        let mut item = StorageItem::new();

        item.deserialize_reader(&mut reader).expect("deserialize");
        assert_eq!(item.value_bytes().as_ref(), data.as_slice());
    }

    #[test]
    fn bigint_roundtrip_through_cache() {
        let original = BigInt::from(-12345);
        let mut item = StorageItem::from_bigint(original.clone());

        // Modify through add_ext
        item.add_ext(BigInt::from(100));

        let result = item.to_bigint();
        assert_eq!(result, BigInt::from(-12245));
    }

    #[test]
    fn large_bigint_serialization() {
        let large_value = BigInt::from(i64::MAX) + BigInt::from(1000);
        let item = StorageItem::from_bigint(large_value.clone());
        assert_eq!(item.to_bigint(), large_value);
    }
}

//! StorageItem - matches C# Neo.SmartContract.StorageItem.
//!
//! The canonical definition now lives in `neo-storage`. This module re-exports
//! it and provides the [`StorageItemExt`] extension trait for cache-aware
//! operations that depend on neo-core types (BigInteger, Interoperable).

use crate::neo_io::serializable::helper::get_var_size_bytes;
use crate::neo_io::{IoResult, MemoryReader};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::interoperable::Interoperable;
use neo_storage::types::storage_item::CacheProvider;
use neo_vm_rs::ExecutionEngineLimits;
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
                Ok(item) => {
                    BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
                        .unwrap_or_default()
                }
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

/// Extension methods on [`StorageItem`] that require neo-core types.
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
        if let Some(cache) = self.cache() {
            if let Some(sc) = cache.as_any().downcast_ref::<StorageCache>() {
                if let StorageCache::BigInteger(v) = sc {
                    return v.clone();
                }
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
        get_var_size_bytes(&self.value_bytes())
    }

    fn deserialize_reader(&mut self, reader: &mut MemoryReader<'_>) -> IoResult<()> {
        let data = reader.read_to_end()?.to_vec();
        self.deserialize_from_bytes(&data);
        Ok(())
    }
}

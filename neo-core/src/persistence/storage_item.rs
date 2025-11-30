//! StorageItem - matches C# Neo.SmartContract.StorageItem.
//!
//! The implementation mirrors the behaviour of `Neo.SmartContract.StorageItem`,
//! including support for cached `BigInteger` and `IInteroperable` payloads,
//! var-size accounting, and replica cloning semantics used across the ledger.

use crate::neo_io::serializable::helper::get_var_size_bytes;
use crate::neo_io::{IoResult, MemoryReader};
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::i_interoperable::IInteroperable;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use num_bigint::{BigInt, Sign};
use std::fmt;

#[allow(dead_code)]
#[derive(Debug)]
enum StorageCache {
    BigInteger(BigInt),
    Interoperable(Box<dyn IInteroperable>),
}

impl Clone for StorageCache {
    fn clone(&self) -> Self {
        match self {
            StorageCache::BigInteger(value) => StorageCache::BigInteger(value.clone()),
            StorageCache::Interoperable(value) => StorageCache::Interoperable(value.clone_box()),
        }
    }
}

/// Represents the values in contract storage (matches C# StorageItem).
#[derive(Debug)]
pub struct StorageItem {
    value: Vec<u8>,
    cache: Option<StorageCache>,
}

impl StorageItem {
    /// Initializes a new instance.
    pub fn new() -> Self {
        Self {
            value: Vec::new(),
            cache: None,
        }
    }

    /// Initializes with a byte-array value.
    pub fn from_bytes(value: Vec<u8>) -> Self {
        Self { value, cache: None }
    }

    /// Initializes with a BigInteger cache.
    pub fn from_bigint(value: BigInt) -> Self {
        Self {
            value: Vec::new(),
            cache: Some(StorageCache::BigInteger(value)),
        }
    }

    /// Returns the encoded size (var-size prefix + payload).
    pub fn size(&self) -> usize {
        get_var_size_bytes(&self.get_value())
    }

    /// Gets the byte-array value.
    pub fn get_value(&self) -> Vec<u8> {
        if !self.value.is_empty() || self.cache.is_none() {
            return self.value.clone();
        }

        match self.cache.as_ref().expect("cache checked above") {
            StorageCache::BigInteger(value) => {
                let (_, bytes) = value.to_bytes_le();
                bytes
            }
            StorageCache::Interoperable(interoperable) => BinarySerializer::serialize(
                &interoperable.to_stack_item(),
                &ExecutionEngineLimits::default(),
            )
            .unwrap_or_default(),
        }
    }

    /// Sets the byte-array value and clears the cache.
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = value;
        self.cache = None;
    }

    /// Ensures the value is serializable and cached.
    pub fn seal(&mut self) {
        if self.value.is_empty() {
            self.value = self.get_value();
        }
    }

    /// Increases the integer value by the specified amount.
    pub fn add(&mut self, integer: BigInt) {
        let current = self.to_bigint();
        self.set_bigint(current + integer);
    }

    /// Copies the value and cache from another instance.
    pub fn from_replica(&mut self, replica: &StorageItem) {
        self.value = replica.value.clone();
        self.cache = replica.cache.clone();
    }

    /// Sets the integer value (clears the current bytes).
    pub fn set_bigint(&mut self, integer: BigInt) {
        self.cache = Some(StorageCache::BigInteger(integer));
        self.value.clear();
    }

    /// Converts the stored value to a `BigInt`.
    pub fn to_bigint(&self) -> BigInt {
        match &self.cache {
            Some(StorageCache::BigInteger(value)) => value.clone(),
            _ => {
                let bytes = self.get_value();
                BigInt::from_bytes_le(Sign::Plus, &bytes)
            }
        }
    }

    /// Deserialize from a memory reader (matches the C# signature).
    pub fn deserialize(&mut self, reader: &mut MemoryReader<'_>) -> IoResult<()> {
        let data = reader.read_to_end()?.to_vec();
        self.value = data;
        self.cache = None;
        Ok(())
    }

    /// Convenience helper to deserialize from raw bytes.
    pub fn deserialize_from_bytes(&mut self, data: &[u8]) {
        self.value = data.to_vec();
        self.cache = None;
    }

    /// Serialize the value to a byte vector.
    pub fn serialize(&self) -> Vec<u8> {
        self.get_value()
    }
}

impl Clone for StorageItem {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            cache: self.cache.clone(),
        }
    }
}

// Use macro to reduce boilerplate
crate::impl_default_via_new!(StorageItem);

impl From<BigInt> for StorageItem {
    fn from(value: BigInt) -> Self {
        Self::from_bigint(value)
    }
}

// Use macro to reduce boilerplate for byte conversions
crate::impl_from_bytes!(StorageItem, owned: from_bytes);

impl fmt::Display for StorageItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value_array = self.get_value();
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

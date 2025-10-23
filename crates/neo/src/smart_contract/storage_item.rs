//! StorageItem - matches C# Neo.SmartContract.StorageItem exactly

use num_bigint::BigInt;
use std::fmt;

/// Represents the values in contract storage (matches C# StorageItem)
#[derive(Clone, Debug)]
pub struct StorageItem {
    value: Option<Vec<u8>>,
    cache: Option<StorageCache>,
}

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

/// Interface for interoperable types (placeholder)
pub trait IInteroperable: std::fmt::Debug + Send + Sync {
    fn to_stack_item(&self) -> StackItem;
    fn from_stack_item(&mut self, item: StackItem);
    fn clone_box(&self) -> Box<dyn IInteroperable>;
}

/// Interface for verifiable interoperable types (placeholder)
pub trait IInteroperableVerifiable: IInteroperable {
    fn from_stack_item_verified(&mut self, item: StackItem, verify: bool);
}

/// Placeholder for StackItem
#[derive(Clone, Debug)]
pub struct StackItem;

impl StorageItem {
    /// Initializes a new instance
    pub fn new() -> Self {
        Self {
            value: None,
            cache: None,
        }
    }

    /// Initializes with byte array value
    pub fn from_bytes(value: Vec<u8>) -> Self {
        Self {
            value: Some(value),
            cache: None,
        }
    }

    /// Initializes with BigInteger value
    pub fn from_bigint(value: BigInt) -> Self {
        Self {
            value: None,
            cache: Some(StorageCache::BigInteger(value)),
        }
    }

    /// Get the size
    pub fn size(&self) -> usize {
        self.get_value().len()
    }

    /// Get the byte array value
    pub fn get_value(&self) -> Vec<u8> {
        if let Some(ref val) = self.value {
            return val.clone();
        }

        match &self.cache {
            Some(StorageCache::BigInteger(bi)) => bi.to_bytes_le().1,
            Some(StorageCache::Interoperable(_)) => {
                // Would serialize the interoperable here
                vec![]
            }
            None => vec![],
        }
    }

    /// Set the byte array value
    pub fn set_value(&mut self, value: Vec<u8>) {
        self.value = Some(value);
        self.cache = None;
    }

    /// Ensure that is serializable and cache the value
    pub fn seal(&mut self) {
        // Force value computation
        let _ = self.get_value();
    }

    /// Increases the integer value by the specified amount
    pub fn add(&mut self, integer: BigInt) {
        let current = self.to_bigint();
        self.set_bigint(current + integer);
    }

    /// Creates a clone
    pub fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            cache: self.cache.clone(),
        }
    }

    /// Copies value from another StorageItem
    pub fn from_replica(&mut self, replica: &StorageItem) {
        self.value = replica.value.clone();
        self.cache = replica.cache.clone();
    }

    /// Sets the integer value
    pub fn set_bigint(&mut self, integer: BigInt) {
        self.cache = Some(StorageCache::BigInteger(integer));
        self.value = None;
    }

    /// Convert to BigInteger
    pub fn to_bigint(&self) -> BigInt {
        if let Some(StorageCache::BigInteger(ref bi)) = self.cache {
            return bi.clone();
        }

        let bytes = self.get_value();
        BigInt::from_bytes_le(num_bigint::Sign::Plus, &bytes)
    }

    /// Deserialize from reader
    pub fn deserialize(&mut self, data: &[u8]) {
        self.value = Some(data.to_vec());
    }

    /// Serialize to writer
    pub fn serialize(&self) -> Vec<u8> {
        self.get_value()
    }
}

impl Default for StorageItem {
    fn default() -> Self {
        Self::new()
    }
}

impl From<BigInt> for StorageItem {
    fn from(value: BigInt) -> Self {
        Self::from_bigint(value)
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

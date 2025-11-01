//! KeyBuilder - matches C# Neo.SmartContract.KeyBuilder exactly

use crate::{UInt160, UInt256};
use num_bigint::BigInt;

/// Used to build storage keys for native contracts (matches C# KeyBuilder)
pub struct KeyBuilder {
    cache_data: Vec<u8>,
    key_length: usize,
}

impl KeyBuilder {
    /// The prefix length (id + prefix byte)
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Initializes a new instance
    pub fn new(id: i32, prefix: u8, max_length: usize) -> Self {
        if max_length == 0 {
            panic!("max_length must be greater than zero");
        }

        let mut cache_data = vec![0u8; max_length + Self::PREFIX_LENGTH];
        cache_data[..4].copy_from_slice(&id.to_le_bytes());
        cache_data[4] = prefix;

        Self {
            cache_data,
            key_length: 5, // sizeof(i32) + sizeof(u8)
        }
    }

    /// Creates with default max length
    pub fn new_with_default(id: i32, prefix: u8) -> Self {
        // ApplicationEngine.MaxStorageKeySize is typically 64
        Self::new(id, prefix, 64)
    }

    fn check_length(&self, length: usize) {
        if self.key_length + length > self.cache_data.len() {
            panic!("Input data too large!");
        }
    }

    /// Adds a byte to the key
    pub fn add_byte(&mut self, key: u8) -> &mut Self {
        self.check_length(1);
        self.cache_data[self.key_length] = key;
        self.key_length += 1;
        self
    }

    /// Adds bytes to the key
    pub fn add(&mut self, key: &[u8]) -> &mut Self {
        self.check_length(key.len());
        self.cache_data[self.key_length..self.key_length + key.len()].copy_from_slice(key);
        self.key_length += key.len();
        self
    }

    /// Adds a UInt160 to the key
    pub fn add_uint160(&mut self, key: &UInt160) -> &mut Self {
        self.add(&key.to_bytes())
    }

    /// Adds a UInt256 to the key
    pub fn add_uint256(&mut self, key: &UInt256) -> &mut Self {
        self.add(&key.to_bytes())
    }

    /// Adds a BigInteger to the key
    pub fn add_big_endian(&mut self, key: &BigInt) -> &mut Self {
        let (sign, bytes) = key.to_bytes_be();
        if sign == num_bigint::Sign::Minus {
            panic!("Cannot add negative BigInteger to key");
        }
        self.add(&bytes)
    }

    /// Converts to StorageKey
    pub fn to_storage_key(&self) -> crate::smart_contract::StorageKey {
        let key_data = &self.cache_data[..self.key_length];
        crate::smart_contract::StorageKey::from_bytes(key_data)
    }

    /// Gets the built key as bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        self.cache_data[..self.key_length].to_vec()
    }

    /// Gets the current key length
    pub fn length(&self) -> usize {
        self.key_length
    }

    /// Adds data with big-endian encoding
    pub fn add_big_endian_bytes(&mut self, value: i32) -> &mut Self {
        self.add(&value.to_be_bytes())
    }
}

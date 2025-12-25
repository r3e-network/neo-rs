//! KeyBuilder - matches C# Neo.SmartContract.KeyBuilder exactly
//!
//! This module provides a builder for constructing storage keys used by native contracts.
//! The implementation mirrors the C# Neo.SmartContract.KeyBuilder class.

use crate::{CoreError, UInt160, UInt256};
use crate::cryptography::ECPoint;
use num_bigint::BigInt;

/// Error type for KeyBuilder operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyBuilderError {
    /// The max_length parameter was zero.
    InvalidMaxLength,
    /// The input data exceeds the maximum key length.
    DataTooLarge {
        current: usize,
        adding: usize,
        max: usize,
    },
    /// Attempted to add a negative BigInteger.
    NegativeBigInteger,
}

impl std::fmt::Display for KeyBuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMaxLength => write!(f, "max_length must be greater than zero"),
            Self::DataTooLarge {
                current,
                adding,
                max,
            } => {
                write!(
                    f,
                    "Input data too large: current={}, adding={}, max={}",
                    current, adding, max
                )
            }
            Self::NegativeBigInteger => write!(f, "Cannot add negative BigInteger to key"),
        }
    }
}

impl std::error::Error for KeyBuilderError {}

impl From<KeyBuilderError> for CoreError {
    fn from(err: KeyBuilderError) -> Self {
        CoreError::invalid_operation(err.to_string())
    }
}

/// Used to build storage keys for native contracts (matches C# KeyBuilder)
pub struct KeyBuilder {
    cache_data: Vec<u8>,
    key_length: usize,
}

impl KeyBuilder {
    /// The prefix length (id + prefix byte)
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Initializes a new instance.
    ///
    pub fn try_new(id: i32, prefix: u8, max_length: usize) -> Result<Self, KeyBuilderError> {
        let mut cache_data = vec![0u8; max_length + Self::PREFIX_LENGTH];
        cache_data[..4].copy_from_slice(&id.to_le_bytes());
        cache_data[4] = prefix;

        Ok(Self {
            cache_data,
            key_length: 5, // sizeof(i32) + sizeof(u8)
        })
    }

    /// Initializes a new instance (panics on invalid input - for backwards compatibility).
    ///
    /// Prefer `try_new` for fallible construction.
    #[inline]
    pub fn new(id: i32, prefix: u8, max_length: usize) -> Self {
        Self::try_new(id, prefix, max_length).expect("KeyBuilder construction failed")
    }

    /// Creates with default max length
    #[inline]
    pub fn new_with_default(id: i32, prefix: u8) -> Self {
        // ApplicationEngine.MaxStorageKeySize is typically 64
        Self::new(id, prefix, 64)
    }

    #[inline]
    fn check_length(&self, length: usize) -> Result<(), KeyBuilderError> {
        if self.key_length + length > self.cache_data.len() {
            return Err(KeyBuilderError::DataTooLarge {
                current: self.key_length,
                adding: length,
                max: self.cache_data.len(),
            });
        }
        Ok(())
    }

    /// Adds a byte to the key.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::DataTooLarge` if the key would exceed max length.
    pub fn try_add_byte(&mut self, key: u8) -> Result<&mut Self, KeyBuilderError> {
        self.check_length(1)?;
        self.cache_data[self.key_length] = key;
        self.key_length += 1;
        Ok(self)
    }

    /// Adds a byte to the key (panics on overflow - for backwards compatibility).
    #[inline]
    pub fn add_byte(&mut self, key: u8) -> &mut Self {
        self.try_add_byte(key).expect("Input data too large")
    }

    /// Adds bytes to the key.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::DataTooLarge` if the key would exceed max length.
    pub fn try_add(&mut self, key: &[u8]) -> Result<&mut Self, KeyBuilderError> {
        self.check_length(key.len())?;
        self.cache_data[self.key_length..self.key_length + key.len()].copy_from_slice(key);
        self.key_length += key.len();
        Ok(self)
    }

    /// Adds bytes to the key (panics on overflow - for backwards compatibility).
    #[inline]
    pub fn add(&mut self, key: &[u8]) -> &mut Self {
        self.try_add(key).expect("Input data too large")
    }

    /// Adds a UInt160 to the key.
    #[inline]
    pub fn add_uint160(&mut self, key: &UInt160) -> &mut Self {
        self.add(&key.to_bytes())
    }

    /// Adds a UInt256 to the key.
    #[inline]
    pub fn add_uint256(&mut self, key: &UInt256) -> &mut Self {
        self.add(&key.to_bytes())
    }

    /// Adds an ECPoint to the key.
    #[inline]
    pub fn add_ecpoint(&mut self, key: &ECPoint) -> &mut Self {
        self.add(key.as_bytes())
    }

    /// Adds a BigInteger to the key.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::NegativeBigInteger` if the value is negative.
    pub fn try_add_big_endian(&mut self, key: &BigInt) -> Result<&mut Self, KeyBuilderError> {
        let (sign, bytes) = key.to_bytes_be();
        if sign == num_bigint::Sign::Minus {
            return Err(KeyBuilderError::NegativeBigInteger);
        }
        self.try_add(&bytes)
    }

    /// Adds a BigInteger to the key (panics on error - for backwards compatibility).
    #[inline]
    pub fn add_big_endian(&mut self, key: &BigInt) -> &mut Self {
        self.try_add_big_endian(key)
            .expect("Cannot add negative BigInteger to key")
    }

    /// Converts to StorageKey
    #[inline]
    pub fn to_storage_key(&self) -> crate::smart_contract::StorageKey {
        let key_data = &self.cache_data[..self.key_length];
        crate::smart_contract::StorageKey::from_bytes(key_data)
    }

    /// Gets the built key as bytes
    #[inline]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.cache_data[..self.key_length].to_vec()
    }

    /// Gets the current key length
    #[inline]
    pub fn length(&self) -> usize {
        self.key_length
    }

    /// Adds data with big-endian encoding
    #[inline]
    pub fn add_big_endian_bytes(&mut self, value: i32) -> &mut Self {
        self.add(&value.to_be_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_try_new_with_zero_max_length() {
        let mut builder = KeyBuilder::try_new(1, 0x01, 0).expect("builder");
        let result = builder.try_add(&[0x01]);
        assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
    }

    #[test]
    fn test_try_add_exceeds_max_length() {
        let mut builder = KeyBuilder::try_new(1, 0x01, 5).unwrap();
        let result = builder.try_add(&[0u8; 10]);
        assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
    }

    #[test]
    fn test_try_add_big_endian_negative() {
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        let result = builder.try_add_big_endian(&BigInt::from(-1));
        assert!(matches!(result, Err(KeyBuilderError::NegativeBigInteger)));
    }
}

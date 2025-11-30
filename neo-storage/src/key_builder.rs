//! KeyBuilder - matches C# Neo.SmartContract.KeyBuilder exactly.
//!
//! This module provides a builder for constructing storage keys used by native contracts.

use crate::types::StorageKey;
use neo_primitives::{UInt160, UInt256};
use std::fmt;

/// Error type for KeyBuilder operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyBuilderError {
    /// The max_length parameter was zero.
    InvalidMaxLength,
    /// The input data exceeds the maximum key length.
    DataTooLarge {
        /// Current key length.
        current: usize,
        /// Bytes being added.
        adding: usize,
        /// Maximum allowed length.
        max: usize,
    },
}

impl fmt::Display for KeyBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMaxLength => write!(f, "max_length must be greater than zero"),
            Self::DataTooLarge { current, adding, max } => {
                write!(f, "Input data too large: current={current}, adding={adding}, max={max}")
            }
        }
    }
}

impl std::error::Error for KeyBuilderError {}

/// Used to build storage keys for native contracts (matches C# KeyBuilder).
pub struct KeyBuilder {
    cache_data: Vec<u8>,
    key_length: usize,
}

impl KeyBuilder {
    /// The prefix length (id + prefix byte).
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Default maximum key size.
    pub const DEFAULT_MAX_LENGTH: usize = 64;

    /// Initializes a new instance.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::InvalidMaxLength` if `max_length` is zero.
    pub fn try_new(id: i32, prefix: u8, max_length: usize) -> Result<Self, KeyBuilderError> {
        if max_length == 0 {
            return Err(KeyBuilderError::InvalidMaxLength);
        }

        let mut cache_data = vec![0u8; max_length + Self::PREFIX_LENGTH];
        cache_data[..4].copy_from_slice(&id.to_le_bytes());
        cache_data[4] = prefix;

        Ok(Self {
            cache_data,
            key_length: Self::PREFIX_LENGTH,
        })
    }

    /// Initializes a new instance (panics on invalid input).
    #[inline]
    pub fn new(id: i32, prefix: u8, max_length: usize) -> Self {
        Self::try_new(id, prefix, max_length).expect("max_length must be greater than zero")
    }

    /// Creates with default max length.
    #[inline]
    pub fn new_with_default(id: i32, prefix: u8) -> Self {
        Self::new(id, prefix, Self::DEFAULT_MAX_LENGTH)
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
    pub fn try_add_byte(&mut self, key: u8) -> Result<&mut Self, KeyBuilderError> {
        self.check_length(1)?;
        self.cache_data[self.key_length] = key;
        self.key_length += 1;
        Ok(self)
    }

    /// Adds a byte to the key (panics on overflow).
    #[inline]
    pub fn add_byte(&mut self, key: u8) -> &mut Self {
        self.try_add_byte(key).expect("Input data too large")
    }

    /// Adds bytes to the key.
    pub fn try_add(&mut self, key: &[u8]) -> Result<&mut Self, KeyBuilderError> {
        self.check_length(key.len())?;
        self.cache_data[self.key_length..self.key_length + key.len()].copy_from_slice(key);
        self.key_length += key.len();
        Ok(self)
    }

    /// Adds bytes to the key (panics on overflow).
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

    /// Adds an i32 in big-endian format.
    #[inline]
    pub fn add_i32_be(&mut self, value: i32) -> &mut Self {
        self.add(&value.to_be_bytes())
    }

    /// Adds a u32 in big-endian format.
    #[inline]
    pub fn add_u32_be(&mut self, value: u32) -> &mut Self {
        self.add(&value.to_be_bytes())
    }

    /// Converts to StorageKey.
    #[inline]
    pub fn to_storage_key(&self) -> StorageKey {
        StorageKey::from_bytes(&self.cache_data[..self.key_length])
    }

    /// Gets the built key as bytes.
    #[inline]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.cache_data[..self.key_length].to_vec()
    }

    /// Gets the current key length.
    #[inline]
    pub fn len(&self) -> usize {
        self.key_length
    }

    /// Returns true if the key is empty (only has prefix).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.key_length == Self::PREFIX_LENGTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_builder_new() {
        let builder = KeyBuilder::new(1, 0x01, 64);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH);
    }

    #[test]
    fn test_key_builder_try_new_zero_max_length() {
        let result = KeyBuilder::try_new(1, 0x01, 0);
        assert!(matches!(result, Err(KeyBuilderError::InvalidMaxLength)));
    }

    #[test]
    fn test_key_builder_add_byte() {
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        builder.add_byte(0x42);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 1);
    }

    #[test]
    fn test_key_builder_add_bytes() {
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        builder.add(&[0x01, 0x02, 0x03]);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 3);
    }

    #[test]
    fn test_key_builder_try_add_exceeds_max_length() {
        let mut builder = KeyBuilder::try_new(1, 0x01, 5).unwrap();
        let result = builder.try_add(&[0u8; 10]);
        assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
    }

    #[test]
    fn test_key_builder_to_bytes() {
        let mut builder = KeyBuilder::new(42, 0xAB, 64);
        builder.add_byte(0xFF);
        let bytes = builder.to_bytes();
        // id (4 bytes LE) + prefix (1 byte) + added byte
        assert_eq!(bytes.len(), 6);
        assert_eq!(&bytes[..4], &42i32.to_le_bytes());
        assert_eq!(bytes[4], 0xAB);
        assert_eq!(bytes[5], 0xFF);
    }

    #[test]
    fn test_key_builder_add_uint160() {
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        let hash = UInt160::zero();
        builder.add_uint160(&hash);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 20);
    }

    #[test]
    fn test_key_builder_add_uint256() {
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        let hash = UInt256::zero();
        builder.add_uint256(&hash);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 32);
    }

    #[test]
    fn test_key_builder_is_empty() {
        let builder = KeyBuilder::new_with_default(1, 0x01);
        assert!(builder.is_empty());

        let mut builder2 = KeyBuilder::new_with_default(1, 0x01);
        builder2.add_byte(0x00);
        assert!(!builder2.is_empty());
    }

    #[test]
    fn test_key_builder_error_display() {
        let err = KeyBuilderError::InvalidMaxLength;
        assert!(err.to_string().contains("greater than zero"));

        let err = KeyBuilderError::DataTooLarge {
            current: 10,
            adding: 20,
            max: 15,
        };
        assert!(err.to_string().contains("10"));
        assert!(err.to_string().contains("20"));
        assert!(err.to_string().contains("15"));
    }
}

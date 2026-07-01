//! `KeyBuilder` - matches C# Neo.SmartContract.KeyBuilder exactly.
//!
//! This module provides a builder for constructing storage keys used by native contracts.

use crate::types::StorageKey;
use neo_primitives::{UInt160, UInt256};
use num_bigint::BigInt;

/// Error type for `KeyBuilder` operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyBuilderError {
    /// The `max_length` parameter was zero.
    #[error("max_length must be greater than zero")]
    InvalidMaxLength,
    /// The input data exceeds the maximum key length.
    #[error("Input data too large: current={current}, adding={adding}, max={max}")]
    DataTooLarge {
        /// Current key length.
        current: usize,
        /// Bytes being added.
        adding: usize,
        /// Maximum allowed length.
        max: usize,
    },
    /// Attempted to add a negative `BigInteger`.
    #[error("Cannot add negative BigInteger to key")]
    NegativeBigInteger,
}

/// Used to build storage keys for native contracts (matches C# `KeyBuilder`).
pub struct KeyBuilder {
    cache_data: Vec<u8>,
    key_length: usize,
}

impl KeyBuilder {
    /// The prefix length (id + prefix byte).
    pub const PREFIX_LENGTH: usize = std::mem::size_of::<i32>() + std::mem::size_of::<u8>();

    /// Default maximum key size.
    pub const DEFAULT_MAX_LENGTH: usize = 64;

    /// Creates a builder with the exact payload capacity.
    ///
    /// This accepts zero payload bytes and is intended for compatibility
    /// adapters that must preserve legacy zero-capacity construction behavior.
    #[must_use]
    pub fn with_payload_capacity(id: i32, prefix: u8, payload_capacity: usize) -> Self {
        let mut cache_data = vec![0u8; payload_capacity + Self::PREFIX_LENGTH];
        cache_data[..4].copy_from_slice(&id.to_le_bytes());
        cache_data[4] = prefix;

        Self {
            cache_data,
            key_length: Self::PREFIX_LENGTH,
        }
    }

    /// Initializes a new instance.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::InvalidMaxLength` if `max_length` is zero.
    pub fn try_new(id: i32, prefix: u8, max_length: usize) -> Result<Self, KeyBuilderError> {
        if max_length == 0 {
            return Err(KeyBuilderError::InvalidMaxLength);
        }

        Ok(Self::with_payload_capacity(id, prefix, max_length))
    }

    /// Initializes a new instance (panics on invalid input).
    #[inline]
    #[must_use]
    pub fn new(id: i32, prefix: u8, max_length: usize) -> Self {
        Self::try_new(id, prefix, max_length).expect("max_length must be greater than zero")
    }

    /// Creates with default max length.
    #[inline]
    #[must_use]
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

    /// Adds a `UInt160` to the key.
    #[inline]
    pub fn add_uint160(&mut self, key: &UInt160) -> &mut Self {
        self.add(&key.to_bytes())
    }

    /// Adds a `UInt256` to the key.
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

    /// Adds a `BigInteger` to the key.
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

    /// Adds a `BigInteger` to the key (panics on error).
    #[inline]
    pub fn add_big_endian(&mut self, key: &BigInt) -> &mut Self {
        self.try_add_big_endian(key)
            .expect("Cannot add negative BigInteger to key")
    }

    /// Converts to `StorageKey`.
    #[inline]
    #[must_use]
    pub fn to_storage_key(&self) -> StorageKey {
        StorageKey::from_bytes(&self.cache_data[..self.key_length])
    }

    /// Gets the built key as a byte slice.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.cache_data[..self.key_length]
    }

    /// Gets the built key as bytes.
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Gets the current key length.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.key_length
    }

    /// Returns true if the key is empty (only has prefix).
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.key_length == Self::PREFIX_LENGTH
    }
}

#[cfg(test)]
#[path = "../tests/core/key_builder.rs"]
mod tests;

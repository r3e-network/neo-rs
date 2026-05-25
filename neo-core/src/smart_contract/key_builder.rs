//! KeyBuilder - matches C# Neo.SmartContract.KeyBuilder exactly.
//!
//! Core keeps the public smart-contract path and core-specific helpers while
//! delegating byte-buffer construction to `neo-storage`.

use crate::cryptography::ECPoint;
use crate::{CoreError, UInt160, UInt256};
use neo_storage::{KeyBuilder as StorageKeyBuilder, KeyBuilderError as StorageKeyBuilderError};
use num_bigint::BigInt;

/// Error type for KeyBuilder operations.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyBuilderError {
    /// The max_length parameter was zero.
    #[error("max_length must be greater than zero")]
    InvalidMaxLength,
    /// The input data exceeds the maximum key length.
    #[error("Input data too large: current={current}, adding={adding}, max={max}")]
    DataTooLarge {
        current: usize,
        adding: usize,
        max: usize,
    },
    /// Attempted to add a negative BigInteger.
    #[error("Cannot add negative BigInteger to key")]
    NegativeBigInteger,
}

impl From<KeyBuilderError> for CoreError {
    fn from(err: KeyBuilderError) -> Self {
        CoreError::invalid_operation(err.to_string())
    }
}

impl From<StorageKeyBuilderError> for KeyBuilderError {
    fn from(err: StorageKeyBuilderError) -> Self {
        match err {
            StorageKeyBuilderError::InvalidMaxLength => Self::InvalidMaxLength,
            StorageKeyBuilderError::DataTooLarge {
                current,
                adding,
                max,
            } => Self::DataTooLarge {
                current,
                adding,
                max,
            },
        }
    }
}

/// Used to build storage keys for native contracts (matches C# KeyBuilder).
pub struct KeyBuilder {
    inner: StorageKeyBuilder,
}

impl KeyBuilder {
    /// The prefix length (id + prefix byte).
    pub const PREFIX_LENGTH: usize = StorageKeyBuilder::PREFIX_LENGTH;

    /// Initializes a new instance.
    pub fn try_new(id: i32, prefix: u8, max_length: usize) -> Result<Self, KeyBuilderError> {
        Ok(Self {
            inner: StorageKeyBuilder::with_payload_capacity(id, prefix, max_length),
        })
    }

    /// Initializes a new instance (panics on invalid input - for backwards compatibility).
    ///
    /// Prefer `try_new` for fallible construction.
    #[inline]
    #[must_use]
    pub fn new(id: i32, prefix: u8, max_length: usize) -> Self {
        Self::try_new(id, prefix, max_length).expect("KeyBuilder construction failed")
    }

    /// Creates with default max length.
    #[inline]
    #[must_use]
    pub fn new_with_default(id: i32, prefix: u8) -> Self {
        Self::new(id, prefix, StorageKeyBuilder::DEFAULT_MAX_LENGTH)
    }

    /// Adds a byte to the key.
    ///
    /// # Errors
    ///
    /// Returns `KeyBuilderError::DataTooLarge` if the key would exceed max length.
    pub fn try_add_byte(&mut self, key: u8) -> Result<&mut Self, KeyBuilderError> {
        self.inner.try_add_byte(key)?;
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
        self.inner.try_add(key)?;
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
        self.add(&key.as_bytes())
    }

    /// Adds a UInt256 to the key.
    #[inline]
    pub fn add_uint256(&mut self, key: &UInt256) -> &mut Self {
        self.add(&key.as_bytes())
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

    /// Converts to StorageKey.
    #[inline]
    #[must_use]
    pub fn to_storage_key(&self) -> crate::smart_contract::StorageKey {
        self.inner.to_storage_key()
    }

    /// Gets the built key as a byte slice (zero-copy).
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.inner.as_bytes()
    }

    /// Gets the built key as an owned byte vector.
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.to_bytes()
    }

    /// Gets the current key length.
    #[inline]
    #[must_use]
    pub fn length(&self) -> usize {
        self.inner.len()
    }

    /// Adds data with big-endian encoding.
    #[inline]
    pub fn add_big_endian_bytes(&mut self, value: i32) -> &mut Self {
        self.inner.add_i32_be(value);
        self
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

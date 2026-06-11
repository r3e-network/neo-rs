//! KeyBuilder - matches C# Neo.SmartContract.KeyBuilder exactly.
//!
//! This is a thin wrapper around [`neo_storage::KeyBuilder`] that adds
//! core-specific helpers (e.g. `add_ecpoint`). All buffer construction and
//! error types live in `neo-storage` as the single source of truth.

use neo_crypto::ECPoint;
use std::ops::{Deref, DerefMut};

pub use neo_storage::KeyBuilderError;

/// Used to build storage keys for native contracts (matches C# KeyBuilder).
///
/// This is a newtype wrapper around [`neo_storage::KeyBuilder`] that adds
/// core-specific methods. All standard key-building methods are available
/// via `Deref`.
pub struct KeyBuilder {
    inner: neo_storage::KeyBuilder,
}

impl Deref for KeyBuilder {
    type Target = neo_storage::KeyBuilder;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for KeyBuilder {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl KeyBuilder {
    /// Byte length of the fixed key prefix (contract id `i32` + prefix `u8`).
    pub const PREFIX_LENGTH: usize = neo_storage::KeyBuilder::PREFIX_LENGTH;

    /// Initializes a new instance.
    pub fn try_new(id: i32, prefix: u8, max_length: usize) -> Result<Self, KeyBuilderError> {
        Ok(Self {
            inner: neo_storage::KeyBuilder::try_new(id, prefix, max_length)?,
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
        Self::new(id, prefix, neo_storage::KeyBuilder::DEFAULT_MAX_LENGTH)
    }

    /// Adds an ECPoint to the key.
    #[inline]
    pub fn add_ecpoint(&mut self, key: &ECPoint) -> &mut Self {
        self.add(key.as_bytes());
        self
    }

    /// Converts to StorageKey.
    #[inline]
    #[must_use]
    pub fn to_storage_key(&self) -> neo_storage::StorageKey {
        self.inner.to_storage_key()
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
    use num_bigint::BigInt;

    #[test]
    fn test_try_new_rejects_max_length_below_prefix() {
        // max_length must accommodate at least the fixed prefix (id + prefix byte);
        // a max_length of 0 cannot, so construction is rejected.
        let result = KeyBuilder::try_new(1, 0x01, 0);
        assert!(matches!(result, Err(KeyBuilderError::InvalidMaxLength)));
    }

    #[test]
    fn test_try_add_rejects_data_exceeding_max_length() {
        // `max_length` is the payload capacity; a 1-byte capacity overflows when
        // more than one payload byte is appended.
        let mut builder = KeyBuilder::try_new(1, 0x01, 1).expect("builder");
        let result = builder.try_add(&[0x01, 0x02]);
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

    #[test]
    fn test_add_ecpoint() {
        use neo_crypto::ECCurve;
        use hex::decode;

        let point_bytes =
            decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .expect("hex");
        let point = ECPoint::decode(&point_bytes, ECCurve::secp256r1()).expect("valid point");
        let mut builder = KeyBuilder::new_with_default(1, 0x01);
        builder.add_ecpoint(&point);
        assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + point.as_bytes().len());
    }
}

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
#[path = "tests/key_builder.rs"]
mod tests;

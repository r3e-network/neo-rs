//! Storage value traits for Neo blockchain.
//!
//! This module provides the `StorageValue` trait that abstracts storage value
//! operations without requiring VM types. This breaks the circular dependency
//! between neo-storage and neo-vm (Chain 1: `StorageItem` → `Interoperable`).
//!
//! # Example
//!
//! ```rust
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use neo_primitives::StorageValue;
//!
//! // Vec<u8> implements StorageValue by default
//! let value = vec![0x01, 0x02, 0x03];
//! let bytes = value.to_storage_bytes();
//! let restored = Vec::<u8>::from_storage_bytes(&bytes)?;
//! assert_eq!(value, restored);
//! # Ok(())
//! # }
//! ```

use thiserror::Error;

/// Errors that can occur during storage operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum StorageValueError {
    /// Deserialization failed.
    #[error("deserialization failed: {message}")]
    DeserializationFailed {
        /// Error message describing the failure.
        message: String,
    },

    /// Invalid data format.
    #[error("invalid data format: {message}")]
    InvalidFormat {
        /// Error message describing the format issue.
        message: String,
    },

    /// Data too large to store.
    #[error("data too large: size={size}, max={max}")]
    DataTooLarge {
        /// Actual size of the data.
        size: usize,
        /// Maximum allowed size.
        max: usize,
    },
}

impl StorageValueError {
    /// Create a deserialization error.
    pub fn deserialization<S: Into<String>>(message: S) -> Self {
        Self::DeserializationFailed {
            message: message.into(),
        }
    }

    /// Create an invalid format error.
    pub fn invalid_format<S: Into<String>>(message: S) -> Self {
        Self::InvalidFormat {
            message: message.into(),
        }
    }

    /// Create a data too large error.
    #[must_use]
    pub const fn data_too_large(size: usize, max: usize) -> Self {
        Self::DataTooLarge { size, max }
    }
}

/// Result type for storage value operations.
pub type StorageValueResult<T> = Result<T, StorageValueError>;

/// Trait for types that can be stored in contract storage.
///
/// This trait abstracts storage serialization without requiring VM types,
/// breaking the circular dependency with `Interoperable` (neo-vm).
///
/// # Design Rationale
///
/// The Neo C# implementation has `StorageItem` that can cache `Interoperable`
/// objects. This creates a dependency from storage to VM types. By using this
/// trait, we can:
///
/// 1. Define a simple `StorageItem<V: StorageValue>` in neo-storage
/// 2. Have neo-core implement `StorageValue` for types that need VM integration
/// 3. Avoid any dependency from neo-storage to neo-vm
///
/// # Performance
///
/// Default implementations use simple byte copies. Custom implementations
/// can optimize for specific data layouts. All methods are marked `#[inline]`
/// to enable monomorphization for hot paths.
pub trait StorageValue: Clone + Send + Sync + 'static {
    /// Serializes the value to bytes for storage.
    ///
    /// # Returns
    ///
    /// A byte vector containing the serialized representation.
    fn to_storage_bytes(&self) -> Vec<u8>;

    /// Deserializes the value from storage bytes.
    ///
    /// # Arguments
    ///
    /// * `data` - The byte slice to deserialize from.
    ///
    /// # Errors
    ///
    /// Returns `StorageValueError` if deserialization fails.
    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self>;

    /// Returns the serialized size in bytes.
    ///
    /// # Note
    ///
    /// This should match the length of `to_storage_bytes()` output.
    /// Default implementation calls `to_storage_bytes().len()` but
    /// custom implementations may optimize this.
    fn storage_size(&self) -> usize {
        self.to_storage_bytes().len()
    }
}

/// Default implementation for byte vectors.
///
/// This is the most common case for storage values - raw bytes.
impl StorageValue for Vec<u8> {
    #[inline]
    fn to_storage_bytes(&self) -> Vec<u8> {
        self.clone()
    }

    #[inline]
    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
        Ok(data.to_vec())
    }

    #[inline]
    fn storage_size(&self) -> usize {
        self.len()
    }
}

/// Implementation for fixed-size byte arrays.
impl<const N: usize> StorageValue for [u8; N] {
    #[inline]
    fn to_storage_bytes(&self) -> Vec<u8> {
        self.to_vec()
    }

    fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
        if data.len() != N {
            return Err(StorageValueError::invalid_format(format!(
                "expected {} bytes, got {}",
                N,
                data.len()
            )));
        }
        let mut arr = [0u8; N];
        arr.copy_from_slice(data);
        Ok(arr)
    }

    #[inline]
    fn storage_size(&self) -> usize {
        N
    }
}

#[cfg(test)]
#[path = "../tests/payload/storage.rs"]
mod tests;

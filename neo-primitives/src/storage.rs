//! Storage value traits for Neo blockchain.
//!
//! This module provides the `IStorageValue` trait that abstracts storage value
//! operations without requiring VM types. This breaks the circular dependency
//! between neo-storage and neo-vm (Chain 1: StorageItem → IInteroperable).
//!
//! # Example
//!
//! ```rust
//! use neo_primitives::IStorageValue;
//!
//! // Vec<u8> implements IStorageValue by default
//! let value = vec![0x01, 0x02, 0x03];
//! let bytes = value.to_storage_bytes();
//! let restored = Vec::<u8>::from_storage_bytes(&bytes).unwrap();
//! assert_eq!(value, restored);
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
    pub fn data_too_large(size: usize, max: usize) -> Self {
        Self::DataTooLarge { size, max }
    }
}

/// Result type for storage value operations.
pub type StorageValueResult<T> = Result<T, StorageValueError>;

/// Trait for types that can be stored in contract storage.
///
/// This trait abstracts storage serialization without requiring VM types,
/// breaking the circular dependency with `IInteroperable` (neo-vm).
///
/// # Design Rationale
///
/// The Neo C# implementation has `StorageItem` that can cache `IInteroperable`
/// objects. This creates a dependency from storage to VM types. By using this
/// trait, we can:
///
/// 1. Define a simple `StorageItem<V: IStorageValue>` in neo-storage
/// 2. Have neo-core implement `IStorageValue` for types that need VM integration
/// 3. Avoid any dependency from neo-storage to neo-vm
///
/// # Performance
///
/// Default implementations use simple byte copies. Custom implementations
/// can optimize for specific data layouts. All methods are marked `#[inline]`
/// to enable monomorphization for hot paths.
pub trait IStorageValue: Clone + Send + Sync + 'static {
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
impl IStorageValue for Vec<u8> {
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
impl<const N: usize> IStorageValue for [u8; N] {
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
mod tests {
    use super::*;

    // ============ StorageValueError Tests ============

    #[test]
    fn test_storage_value_error_deserialization() {
        let err = StorageValueError::deserialization("test error");
        assert!(err.to_string().contains("deserialization failed"));
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_storage_value_error_invalid_format() {
        let err = StorageValueError::invalid_format("bad format");
        assert!(err.to_string().contains("invalid data format"));
        assert!(err.to_string().contains("bad format"));
    }

    #[test]
    fn test_storage_value_error_data_too_large() {
        let err = StorageValueError::data_too_large(100, 50);
        assert!(err.to_string().contains("data too large"));
        assert!(err.to_string().contains("size=100"));
        assert!(err.to_string().contains("max=50"));
    }

    #[test]
    fn test_storage_value_error_clone() {
        let err1 = StorageValueError::deserialization("test");
        let err2 = err1.clone();
        assert_eq!(err1, err2);
    }

    #[test]
    fn test_storage_value_error_debug() {
        let err = StorageValueError::deserialization("test");
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("DeserializationFailed"));
    }

    // ============ Vec<u8> IStorageValue Tests ============

    #[test]
    fn test_vec_u8_to_storage_bytes() {
        let value = vec![0x01, 0x02, 0x03, 0x04];
        let bytes = value.to_storage_bytes();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_vec_u8_from_storage_bytes() {
        let data = &[0xAA, 0xBB, 0xCC];
        let restored = Vec::<u8>::from_storage_bytes(data).unwrap();
        assert_eq!(restored, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn test_vec_u8_storage_size() {
        let value = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        assert_eq!(value.storage_size(), 5);
    }

    #[test]
    fn test_vec_u8_empty() {
        let value: Vec<u8> = vec![];
        let bytes = value.to_storage_bytes();
        assert!(bytes.is_empty());

        let restored = Vec::<u8>::from_storage_bytes(&[]).unwrap();
        assert!(restored.is_empty());

        assert_eq!(value.storage_size(), 0);
    }

    #[test]
    fn test_vec_u8_roundtrip() {
        let original = vec![0x00, 0xFF, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let bytes = original.to_storage_bytes();
        let restored = Vec::<u8>::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_vec_u8_large_value() {
        let original: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        let bytes = original.to_storage_bytes();
        let restored = Vec::<u8>::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original, restored);
        assert_eq!(original.storage_size(), 1000);
    }

    // ============ [u8; N] IStorageValue Tests ============

    #[test]
    fn test_fixed_array_to_storage_bytes() {
        let value: [u8; 4] = [0x01, 0x02, 0x03, 0x04];
        let bytes = value.to_storage_bytes();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_fixed_array_from_storage_bytes() {
        let data = &[0xAA, 0xBB, 0xCC, 0xDD];
        let restored = <[u8; 4]>::from_storage_bytes(data).unwrap();
        assert_eq!(restored, [0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_fixed_array_storage_size() {
        let value: [u8; 32] = [0u8; 32];
        assert_eq!(value.storage_size(), 32);
    }

    #[test]
    fn test_fixed_array_wrong_size() {
        let data = &[0x01, 0x02, 0x03]; // 3 bytes
        let result = <[u8; 4]>::from_storage_bytes(data); // expects 4 bytes
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, StorageValueError::InvalidFormat { .. }));
    }

    #[test]
    fn test_fixed_array_roundtrip() {
        let original: [u8; 20] = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
            0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13,
        ];
        let bytes = original.to_storage_bytes();
        let restored = <[u8; 20]>::from_storage_bytes(&bytes).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_fixed_array_zero_size() {
        let value: [u8; 0] = [];
        let bytes = value.to_storage_bytes();
        assert!(bytes.is_empty());

        let restored = <[u8; 0]>::from_storage_bytes(&[]).unwrap();
        assert_eq!(restored.len(), 0);
    }

    // ============ Custom Implementation Test ============

    /// A mock struct to demonstrate custom IStorageValue implementation.
    #[derive(Clone, Debug, PartialEq)]
    struct MockStorageValue {
        id: u32,
        data: Vec<u8>,
    }

    impl IStorageValue for MockStorageValue {
        fn to_storage_bytes(&self) -> Vec<u8> {
            let mut bytes = Vec::with_capacity(4 + self.data.len());
            bytes.extend_from_slice(&self.id.to_le_bytes());
            bytes.extend_from_slice(&self.data);
            bytes
        }

        fn from_storage_bytes(data: &[u8]) -> StorageValueResult<Self> {
            if data.len() < 4 {
                return Err(StorageValueError::invalid_format(
                    "data too short for MockStorageValue",
                ));
            }
            let id = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let payload = data[4..].to_vec();
            Ok(MockStorageValue { id, data: payload })
        }

        fn storage_size(&self) -> usize {
            4 + self.data.len()
        }
    }

    #[test]
    fn test_custom_impl_roundtrip() {
        let original = MockStorageValue {
            id: 12345,
            data: vec![0xAA, 0xBB, 0xCC],
        };

        let bytes = original.to_storage_bytes();
        let restored = MockStorageValue::from_storage_bytes(&bytes).unwrap();

        assert_eq!(original, restored);
        assert_eq!(original.storage_size(), 7); // 4 + 3
    }

    #[test]
    fn test_custom_impl_error() {
        let short_data = &[0x01, 0x02]; // Too short
        let result = MockStorageValue::from_storage_bytes(short_data);
        assert!(result.is_err());
    }

    // ============ Trait Object Tests ============

    #[test]
    fn test_trait_object_vec() {
        fn use_storage_value<V: IStorageValue>(value: &V) -> Vec<u8> {
            value.to_storage_bytes()
        }

        let value = vec![0x01, 0x02, 0x03];
        let bytes = use_storage_value(&value);
        assert_eq!(bytes, vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn test_trait_object_fixed_array() {
        fn use_storage_value<V: IStorageValue>(value: &V) -> usize {
            value.storage_size()
        }

        let value: [u8; 10] = [0u8; 10];
        let size = use_storage_value(&value);
        assert_eq!(size, 10);
    }

    // ============ Send + Sync Tests ============

    #[test]
    fn test_vec_u8_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Vec<u8>>();
    }

    #[test]
    fn test_fixed_array_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<[u8; 32]>();
    }

    // ============ Additional Coverage Tests ============

    #[test]
    fn test_storage_value_error_all_variants_eq() {
        // Test PartialEq for all variants
        let err1 = StorageValueError::DeserializationFailed {
            message: "test".to_string(),
        };
        let err2 = StorageValueError::DeserializationFailed {
            message: "test".to_string(),
        };
        let err3 = StorageValueError::DeserializationFailed {
            message: "other".to_string(),
        };
        assert_eq!(err1, err2);
        assert_ne!(err1, err3);

        let err4 = StorageValueError::InvalidFormat {
            message: "fmt".to_string(),
        };
        let err5 = StorageValueError::InvalidFormat {
            message: "fmt".to_string(),
        };
        assert_eq!(err4, err5);
        assert_ne!(err1, err4);

        let err6 = StorageValueError::DataTooLarge { size: 10, max: 5 };
        let err7 = StorageValueError::DataTooLarge { size: 10, max: 5 };
        let err8 = StorageValueError::DataTooLarge { size: 20, max: 5 };
        assert_eq!(err6, err7);
        assert_ne!(err6, err8);
    }

    #[test]
    fn test_fixed_array_various_sizes() {
        // Test different array sizes
        let arr8: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(arr8.storage_size(), 8);
        let bytes = arr8.to_storage_bytes();
        let restored = <[u8; 8]>::from_storage_bytes(&bytes).unwrap();
        assert_eq!(arr8, restored);

        let arr32: [u8; 32] = [0xAB; 32];
        assert_eq!(arr32.storage_size(), 32);
        let bytes32 = arr32.to_storage_bytes();
        let restored32 = <[u8; 32]>::from_storage_bytes(&bytes32).unwrap();
        assert_eq!(arr32, restored32);
    }

    #[test]
    fn test_fixed_array_size_mismatch_errors() {
        // Test size mismatch error paths
        let result4_from_5 = <[u8; 4]>::from_storage_bytes(&[1, 2, 3, 4, 5]);
        assert!(matches!(
            result4_from_5,
            Err(StorageValueError::InvalidFormat { .. })
        ));

        let result8_from_4 = <[u8; 8]>::from_storage_bytes(&[1, 2, 3, 4]);
        assert!(matches!(
            result8_from_4,
            Err(StorageValueError::InvalidFormat { .. })
        ));

        let result32_from_0 = <[u8; 32]>::from_storage_bytes(&[]);
        assert!(matches!(
            result32_from_0,
            Err(StorageValueError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn test_default_storage_size_impl() {
        // Test that default storage_size matches to_storage_bytes().len()
        let value = MockStorageValue {
            id: 999,
            data: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(value.storage_size(), value.to_storage_bytes().len());
    }
}

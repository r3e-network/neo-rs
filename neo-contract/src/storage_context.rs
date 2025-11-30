//! StorageContext - matches C# Neo.SmartContract.StorageContext exactly.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The storage context used to read and write data in smart contracts.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageContext {
    /// The id of the contract that owns the context.
    pub id: i32,
    /// Indicates whether the context is read-only.
    pub is_read_only: bool,
}

impl StorageContext {
    /// Creates a new storage context.
    pub fn new(id: i32, is_read_only: bool) -> Self {
        Self { id, is_read_only }
    }

    /// Creates a read-only storage context.
    pub fn read_only(id: i32) -> Self {
        Self {
            id,
            is_read_only: true,
        }
    }

    /// Creates a read-write storage context.
    pub fn read_write(id: i32) -> Self {
        Self {
            id,
            is_read_only: false,
        }
    }

    /// Converts to read-only context.
    pub fn as_read_only(&self) -> Self {
        Self {
            id: self.id,
            is_read_only: true,
        }
    }

    /// Encodes the storage context as bytes (id + read-only flag) matching C# serialization.
    pub fn to_bytes(&self) -> [u8; 5] {
        let mut data = [0u8; 5];
        data[..4].copy_from_slice(&self.id.to_le_bytes());
        data[4] = u8::from(self.is_read_only);
        data
    }

    /// Builds a storage context from encoded bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, StorageContextError> {
        if bytes.len() != 5 {
            return Err(StorageContextError::InvalidLength(bytes.len()));
        }

        let mut id_bytes = [0u8; 4];
        id_bytes.copy_from_slice(&bytes[..4]);
        let id = i32::from_le_bytes(id_bytes);
        let is_read_only = match bytes[4] {
            0 => false,
            1 => true,
            flag => return Err(StorageContextError::InvalidFlag(flag)),
        };

        Ok(Self { id, is_read_only })
    }
}

impl Default for StorageContext {
    fn default() -> Self {
        Self {
            id: 0,
            is_read_only: false,
        }
    }
}

impl fmt::Display for StorageContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "StorageContext(id={}, read_only={})",
            self.id, self.is_read_only
        )
    }
}

/// Errors that can occur when working with StorageContext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageContextError {
    /// Invalid byte length for deserialization.
    InvalidLength(usize),
    /// Invalid read-only flag value.
    InvalidFlag(u8),
}

impl fmt::Display for StorageContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength(len) => {
                write!(f, "StorageContext payload must be 5 bytes, got {len}")
            }
            Self::InvalidFlag(flag) => {
                write!(f, "Invalid StorageContext read-only flag: {flag}")
            }
        }
    }
}

impl std::error::Error for StorageContextError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_context_new() {
        let ctx = StorageContext::new(42, true);
        assert_eq!(ctx.id, 42);
        assert!(ctx.is_read_only);
    }

    #[test]
    fn test_storage_context_read_only() {
        let ctx = StorageContext::read_only(10);
        assert_eq!(ctx.id, 10);
        assert!(ctx.is_read_only);
    }

    #[test]
    fn test_storage_context_read_write() {
        let ctx = StorageContext::read_write(20);
        assert_eq!(ctx.id, 20);
        assert!(!ctx.is_read_only);
    }

    #[test]
    fn test_storage_context_as_read_only() {
        let ctx = StorageContext::read_write(30);
        let read_only = ctx.as_read_only();
        assert_eq!(read_only.id, 30);
        assert!(read_only.is_read_only);
    }

    #[test]
    fn test_storage_context_to_bytes() {
        let ctx = StorageContext::new(0x12345678, true);
        let bytes = ctx.to_bytes();
        assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12, 1]);
    }

    #[test]
    fn test_storage_context_from_bytes() {
        let bytes = [0x78, 0x56, 0x34, 0x12, 0];
        let ctx = StorageContext::from_bytes(&bytes).unwrap();
        assert_eq!(ctx.id, 0x12345678);
        assert!(!ctx.is_read_only);
    }

    #[test]
    fn test_storage_context_roundtrip() {
        let original = StorageContext::new(-100, true);
        let bytes = original.to_bytes();
        let recovered = StorageContext::from_bytes(&bytes).unwrap();
        assert_eq!(original, recovered);
    }

    #[test]
    fn test_storage_context_from_bytes_invalid_length() {
        let bytes = [0, 1, 2];
        let result = StorageContext::from_bytes(&bytes);
        assert!(matches!(result, Err(StorageContextError::InvalidLength(3))));
    }

    #[test]
    fn test_storage_context_from_bytes_invalid_flag() {
        let bytes = [0, 0, 0, 0, 2];
        let result = StorageContext::from_bytes(&bytes);
        assert!(matches!(result, Err(StorageContextError::InvalidFlag(2))));
    }

    #[test]
    fn test_storage_context_display() {
        let ctx = StorageContext::new(42, true);
        assert_eq!(ctx.to_string(), "StorageContext(id=42, read_only=true)");
    }

    #[test]
    fn test_storage_context_default() {
        let ctx = StorageContext::default();
        assert_eq!(ctx.id, 0);
        assert!(!ctx.is_read_only);
    }
}

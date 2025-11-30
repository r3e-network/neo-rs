//! Error types for storage operations.

use thiserror::Error;

/// Errors that can occur during storage operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// Key not found in storage.
    #[error("Key not found: {key}")]
    KeyNotFound {
        /// The key that was not found.
        key: String,
    },

    /// Storage is read-only.
    #[error("Storage is read-only")]
    ReadOnly,

    /// Serialization/deserialization error.
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error message.
        message: String,
    },

    /// Backend-specific error.
    #[error("Storage backend error: {message}")]
    Backend {
        /// Error message from the backend.
        message: String,
    },

    /// Invalid operation.
    #[error("Invalid operation: {message}")]
    InvalidOperation {
        /// Error message.
        message: String,
    },
}

impl StorageError {
    /// Create a key not found error.
    pub fn key_not_found<S: Into<String>>(key: S) -> Self {
        Self::KeyNotFound { key: key.into() }
    }

    /// Create a serialization error.
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        Self::Serialization {
            message: message.into(),
        }
    }

    /// Create a backend error.
    pub fn backend<S: Into<String>>(message: S) -> Self {
        Self::Backend {
            message: message.into(),
        }
    }

    /// Create an invalid operation error.
    pub fn invalid_operation<S: Into<String>>(message: S) -> Self {
        Self::InvalidOperation {
            message: message.into(),
        }
    }
}

/// Result type for storage operations.
pub type StorageResult<T> = std::result::Result<T, StorageError>;

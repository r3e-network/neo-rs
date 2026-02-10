use neo_io::IoError;
use neo_primitives::PrimitiveError;
use thiserror::Error;

/// Result type alias for MPT trie operations.
pub type MptResult<T> = Result<T, MptError>;

/// Errors that can occur during Merkle Patricia Trie operations.
#[derive(Debug, Error)]
pub enum MptError {
    /// An I/O error occurred during serialization or deserialization.
    #[error("IO error: {0}")]
    Io(#[from] IoError),
    /// A primitive type conversion error.
    #[error("primitive error: {0}")]
    Primitive(#[from] PrimitiveError),
    /// A storage backend error.
    #[error("storage error: {0}")]
    Storage(String),
    /// An invalid operation was attempted on the trie.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
    /// A key-related error (e.g. invalid length or encoding).
    #[error("key error: {0}")]
    Key(String),
}

impl MptError {
    /// Creates a storage error with the given message.
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }

    /// Creates an invalid-operation error with the given message.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }

    /// Creates a key error with the given message.
    pub fn key(message: impl Into<String>) -> Self {
        Self::Key(message.into())
    }
}

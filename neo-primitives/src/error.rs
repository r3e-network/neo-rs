//! Error types for Neo primitives.

use thiserror::Error;

/// Errors that can occur when working with primitive types.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum PrimitiveError {
    /// Invalid format error with detailed description.
    #[error("Invalid format: {message}")]
    InvalidFormat {
        /// Error message describing the format issue.
        message: String,
    },

    /// Invalid data error with context.
    #[error("Invalid data: {message}")]
    InvalidData {
        /// Error message describing the data issue.
        message: String,
    },

    /// Buffer overflow or underflow.
    #[error("Buffer overflow: attempted to read {requested} bytes, but only {available} available")]
    BufferOverflow {
        /// Amount of space requested.
        requested: usize,
        /// Amount of space available.
        available: usize,
    },

    /// Type conversion failed.
    #[error("Type conversion failed: cannot convert {from} to {to}")]
    TypeConversion {
        /// Source type name.
        from: String,
        /// Target type name.
        to: String,
    },
}

impl PrimitiveError {
    /// Create a new invalid format error.
    pub fn invalid_format<S: Into<String>>(message: S) -> Self {
        Self::InvalidFormat {
            message: message.into(),
        }
    }

    /// Create a new invalid data error.
    pub fn invalid_data<S: Into<String>>(message: S) -> Self {
        Self::InvalidData {
            message: message.into(),
        }
    }

    /// Create a new buffer overflow error.
    #[must_use]
    pub const fn buffer_overflow(requested: usize, available: usize) -> Self {
        Self::BufferOverflow {
            requested,
            available,
        }
    }

    /// Create a new type conversion error.
    pub fn type_conversion<S: Into<String>>(from: S, to: S) -> Self {
        Self::TypeConversion {
            from: from.into(),
            to: to.into(),
        }
    }
}

/// Result type for primitive operations.
pub type PrimitiveResult<T> = std::result::Result<T, PrimitiveError>;

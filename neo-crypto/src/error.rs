//! Error types for cryptographic operations.
//!
//! This module defines the error types used throughout the crypto crate for
//! handling failures in cryptographic operations.
//!
//! ## Error Types
//!
//! | Error | Description |
//! |-------|-------------|
//! | `InvalidArgument` | Invalid input parameters |
//! | `InvalidKey` | Malformed or invalid key |
//! | `InvalidSignature` | Invalid signature format or verification failure |
//! | `InvalidPoint` | Invalid elliptic curve point |
//! | `HashError` | Hash computation failure |
//! | `EncodingError` | Base58, hex, or other encoding errors |
//!
//! ## Example
//!
//! ```rust
//! use neo_crypto::error::CryptoError;
//!
//! // Create an error
//! let err = CryptoError::invalid_key("key too short");
//!
//! // Convert to string
//! assert!(err.to_string().contains("Invalid key"));
//! ```

use thiserror::Error;

/// Errors that can occur during cryptographic operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// Invalid argument provided to a cryptographic utility.
    #[error("Invalid argument: {message}")]
    InvalidArgument {
        /// Error message describing the argument issue.
        message: String,
    },

    /// Invalid key format or length.
    #[error("Invalid key: {message}")]
    InvalidKey {
        /// Error message describing the key issue.
        message: String,
    },

    /// Invalid signature format or verification failed.
    #[error("Invalid signature: {message}")]
    InvalidSignature {
        /// Error message describing the signature issue.
        message: String,
    },

    /// Invalid point on elliptic curve.
    #[error("Invalid curve point: {message}")]
    InvalidPoint {
        /// Error message describing the point issue.
        message: String,
    },

    /// Hash computation failed.
    #[error("Hash error: {message}")]
    HashError {
        /// Error message describing the hash issue.
        message: String,
    },

    /// Encoding/decoding error.
    #[error("Encoding error: {message}")]
    EncodingError {
        /// Error message describing the encoding issue.
        message: String,
    },
}

impl CryptoError {
    /// Create a new invalid argument error.
    pub fn invalid_argument<S: Into<String>>(message: S) -> Self {
        Self::InvalidArgument {
            message: message.into(),
        }
    }

    /// Create a new invalid key error.
    pub fn invalid_key<S: Into<String>>(message: S) -> Self {
        Self::InvalidKey {
            message: message.into(),
        }
    }

    /// Create a new invalid signature error.
    pub fn invalid_signature<S: Into<String>>(message: S) -> Self {
        Self::InvalidSignature {
            message: message.into(),
        }
    }

    /// Create a new invalid point error.
    pub fn invalid_point<S: Into<String>>(message: S) -> Self {
        Self::InvalidPoint {
            message: message.into(),
        }
    }

    /// Create a new encoding error.
    pub fn encoding_error<S: Into<String>>(message: S) -> Self {
        Self::EncodingError {
            message: message.into(),
        }
    }
}

/// Result type for cryptographic operations.
pub type CryptoResult<T> = std::result::Result<T, CryptoError>;

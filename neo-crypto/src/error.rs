//! Error types for cryptographic operations.

use thiserror::Error;

/// Errors that can occur during cryptographic operations.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
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

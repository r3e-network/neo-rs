//! Error types for cryptographic operations.

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

    /// Key generation failed after exhausting retry budget.
    #[error("Key generation failed: {message}")]
    KeyGenerationFailed {
        /// Error message describing the generation failure.
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

    /// Create a new key generation failure error.
    pub fn key_generation_failed<S: Into<String>>(message: S) -> Self {
        Self::KeyGenerationFailed {
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// From impls for underlying library error types
// ---------------------------------------------------------------------------

impl From<secp256k1::Error> for CryptoError {
    fn from(e: secp256k1::Error) -> Self {
        // secp256k1::Error covers invalid keys, signatures, and messages.
        // Map to the most general variant; callers that need finer granularity
        // should use map_err with a specific constructor instead.
        Self::InvalidKey {
            message: e.to_string(),
        }
    }
}

// p256::ecdsa::Error is a re-export of `signature::Error`, which is also
// `ed25519_dalek::SignatureError`. A single From impl covers both.
impl From<p256::ecdsa::Error> for CryptoError {
    fn from(e: p256::ecdsa::Error) -> Self {
        Self::InvalidSignature {
            message: e.to_string(),
        }
    }
}

impl From<bs58::decode::Error> for CryptoError {
    fn from(e: bs58::decode::Error) -> Self {
        Self::EncodingError {
            message: format!("Base58 decode error: {e}"),
        }
    }
}

impl From<hex::FromHexError> for CryptoError {
    fn from(e: hex::FromHexError) -> Self {
        Self::EncodingError {
            message: format!("Hex decode error: {e}"),
        }
    }
}

/// Result type for cryptographic operations.
pub type CryptoResult<T> = std::result::Result<T, CryptoError>;

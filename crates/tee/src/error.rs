//! TEE Error types

use thiserror::Error;

/// Result type for TEE operations
pub type TeeResult<T> = std::result::Result<T, TeeError>;

/// TEE-specific errors
#[derive(Error, Debug)]
pub enum TeeError {
    #[error("Enclave not initialized")]
    EnclaveNotInitialized,

    #[error("Enclave initialization failed: {0}")]
    EnclaveInitFailed(String),

    #[error("Sealing failed: {0}")]
    SealingFailed(String),

    #[error("Unsealing failed: {0}")]
    UnsealingFailed(String),

    #[error("Attestation failed: {0}")]
    AttestationFailed(String),

    #[error("Invalid attestation report")]
    InvalidAttestationReport,

    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    #[error("Key not found: {0}")]
    KeyNotFound(String),

    #[error("Invalid key format")]
    InvalidKeyFormat,

    #[error("Transaction ordering error: {0}")]
    OrderingError(String),

    #[error("Mempool capacity exceeded")]
    MempoolFull,

    #[error("SGX hardware not available")]
    SgxNotAvailable,

    #[error("Feature not enabled: {0}")]
    FeatureNotEnabled(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Other error: {0}")]
    Other(String),
}

impl From<serde_json::Error> for TeeError {
    fn from(e: serde_json::Error) -> Self {
        TeeError::SerializationError(e.to_string())
    }
}

impl From<hex::FromHexError> for TeeError {
    fn from(e: hex::FromHexError) -> Self {
        TeeError::InvalidKeyFormat
    }
}

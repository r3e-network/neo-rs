//! TEE Error types

use thiserror::Error;

/// Result type for TEE operations
pub type TeeResult<T> = std::result::Result<T, TeeError>;

/// Enclave initialization error details
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EnclaveInitError {
    /// Enclave is already initialized
    #[error("enclave already initialized")]
    AlreadyInitialized,
    /// Sealed data directory cannot be created
    #[error("failed to create sealed data directory")]
    DirectoryCreationFailed,
    /// Failed to derive sealing key
    #[error("failed to derive sealing key")]
    SealingKeyDerivationFailed,
    /// Failed to load monotonic counter
    #[error("failed to load monotonic counter")]
    CounterLoadFailed,
    /// Invalid configuration
    #[error("invalid enclave configuration")]
    InvalidConfiguration,
    /// Hardware TEE not available
    #[error("hardware TEE not available")]
    HardwareUnavailable,
    /// Debug mode not allowed in production
    #[error("debug mode not allowed in production")]
    DebugNotAllowed,
}

/// TEE-specific errors
#[derive(Error, Debug)]
pub enum TeeError {
    #[error("Enclave not initialized")]
    EnclaveNotInitialized,

    #[error("Enclave initialization failed: {0}")]
    EnclaveInitFailed(String),

    #[error("Enclave initialization error: {error}. context: {context}")]
    EnclaveInitError {
        /// The specific initialization error
        error: EnclaveInitError,
        /// Additional context
        context: String,
    },

    #[error("Sealing failed: {0}")]
    SealingFailed(String),

    #[error("Unsealing failed: {0}")]
    UnsealingFailed(String),

    #[error("Attestation failed: {0}")]
    AttestationFailed(String),

    #[error("Invalid attestation report: {0}")]
    InvalidAttestationReport(String),

    #[error("Quote validation failed: {0}")]
    QuoteValidationFailed(String),

    #[error("MRENCLAVE verification failed: expected {expected}, got {actual}")]
    MrEnclaveMismatch { expected: String, actual: String },

    #[error("MRSIGNER verification failed: expected {expected}, got {actual}")]
    MrSignerMismatch { expected: String, actual: String },

    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),

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

impl TeeError {
    /// Create a new enclave initialization error with context
    pub fn enclave_init_error(error: EnclaveInitError, context: impl Into<String>) -> Self {
        TeeError::EnclaveInitError {
            error,
            context: context.into(),
        }
    }

    /// Create a MRENCLAVE mismatch error
    pub fn mrenclave_mismatch(expected: &[u8; 32], actual: &[u8; 32]) -> Self {
        TeeError::MrEnclaveMismatch {
            expected: hex::encode(expected),
            actual: hex::encode(actual),
        }
    }

    /// Create a MRSIGNER mismatch error
    pub fn mrsigner_mismatch(expected: &[u8; 32], actual: &[u8; 32]) -> Self {
        TeeError::MrSignerMismatch {
            expected: hex::encode(expected),
            actual: hex::encode(actual),
        }
    }
}

impl From<serde_json::Error> for TeeError {
    fn from(e: serde_json::Error) -> Self {
        TeeError::SerializationError(e.to_string())
    }
}

impl From<hex::FromHexError> for TeeError {
    fn from(_e: hex::FromHexError) -> Self {
        TeeError::InvalidKeyFormat
    }
}

// EnclaveInitError is defined above and re-exported from enclave module

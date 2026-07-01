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
    /// Operation requires an initialized enclave, but initialization has not completed.
    #[error("Enclave not initialized")]
    EnclaveNotInitialized,

    /// Enclave initialization failed with a human-readable reason.
    #[error("Enclave initialization failed: {0}")]
    EnclaveInitFailed(String),

    /// Enclave initialization failed with a typed error and contextual details.
    #[error("Enclave initialization error: {error}. context: {context}")]
    EnclaveInitError {
        /// The specific initialization error
        error: EnclaveInitError,
        /// Additional context
        context: String,
    },

    /// Sealing plaintext into enclave-protected data failed.
    #[error("Sealing failed: {0}")]
    SealingFailed(String),

    /// Unsealing enclave-protected data failed.
    #[error("Unsealing failed: {0}")]
    UnsealingFailed(String),

    /// Hardware or simulated attestation failed.
    #[error("Attestation failed: {0}")]
    AttestationFailed(String),

    /// Attestation report structure or contents were invalid.
    #[error("Invalid attestation report: {0}")]
    InvalidAttestationReport(String),

    /// SGX quote validation failed.
    #[error("Quote validation failed: {0}")]
    QuoteValidationFailed(String),

    /// Verified enclave measurement did not match the expected MRENCLAVE value.
    #[error("MRENCLAVE verification failed: expected {expected}, got {actual}")]
    MrEnclaveMismatch {
        /// Expected MRENCLAVE measurement encoded as hex.
        expected: String,
        /// Actual MRENCLAVE measurement encoded as hex.
        actual: String,
    },

    /// Verified signer measurement did not match the expected MRSIGNER value.
    #[error("MRSIGNER verification failed: expected {expected}, got {actual}")]
    MrSignerMismatch {
        /// Expected MRSIGNER measurement encoded as hex.
        expected: String,
        /// Actual MRSIGNER measurement encoded as hex.
        actual: String,
    },

    /// Cryptographic operation failed.
    #[error("Cryptographic error: {0}")]
    CryptoError(String),

    /// Key derivation failed.
    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),

    /// Requested enclave-managed key was not found.
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Key material was malformed or had an unexpected length.
    #[error("Invalid key format")]
    InvalidKeyFormat,

    /// Transaction ordering or monotonic sequencing validation failed.
    #[error("Transaction ordering error: {0}")]
    OrderingError(String),

    /// Enclave mempool reached its configured capacity.
    #[error("Mempool capacity exceeded")]
    MempoolFull,

    /// SGX hardware support is required but unavailable.
    #[error("SGX hardware not available")]
    SgxNotAvailable,

    /// Requested TEE feature is not enabled in this build.
    #[error("Feature not enabled: {0}")]
    FeatureNotEnabled(String),

    /// Serialization or deserialization failed.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Underlying IO operation failed.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Miscellaneous TEE error that does not fit a more specific variant.
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

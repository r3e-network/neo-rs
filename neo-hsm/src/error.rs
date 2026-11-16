//! HSM Error types

use thiserror::Error;

/// Result type for HSM operations
pub type HsmResult<T> = std::result::Result<T, HsmError>;

/// HSM error types
#[derive(Error, Debug)]
pub enum HsmError {
    /// HSM has not been initialized
    #[error("HSM not initialized")]
    NotInitialized,

    /// HSM initialization failed
    #[error("HSM initialization failed: {0}")]
    InitFailed(String),

    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Device communication error
    #[error("Device communication error: {0}")]
    DeviceError(String),

    /// PIN is required but not provided
    #[error("PIN required")]
    PinRequired,

    /// Invalid PIN provided
    #[error("Invalid PIN")]
    InvalidPin,

    /// PIN is locked due to too many failed attempts
    #[error("PIN locked (too many attempts)")]
    PinLocked,

    /// Key not found in HSM
    #[error("Key not found: {0}")]
    KeyNotFound(String),

    /// Signing operation failed
    #[error("Signing failed: {0}")]
    SigningFailed(String),

    /// User rejected the operation on device
    #[error("User rejected operation")]
    UserRejected,

    /// PKCS#11 specific error
    #[error("PKCS#11 error: {0}")]
    Pkcs11Error(String),

    /// Ledger specific error
    #[error("Ledger error: {0}")]
    LedgerError(String),

    /// Operation not supported by this HSM
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// Feature not enabled at compile time
    #[error("Feature not enabled: {0}")]
    FeatureNotEnabled(String),

    /// Invalid derivation path
    #[error("Invalid derivation path: {0}")]
    InvalidDerivationPath(String),

    /// Invalid key format
    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),

    /// Timeout waiting for device
    #[error("Device timeout: {0}")]
    Timeout(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Cryptographic error
    #[error("Crypto error: {0}")]
    CryptoError(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

impl From<anyhow::Error> for HsmError {
    fn from(err: anyhow::Error) -> Self {
        HsmError::Other(err.to_string())
    }
}

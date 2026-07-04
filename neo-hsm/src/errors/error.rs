//! Error types for HSM operations.
//!
//! All HSM errors map to [`neo_consensus::error::ConsensusError`] via
//! [`From<HsmError>`] using `ConsensusError::state_error`, so the consensus
//! driver receives an opaque string and triggers change-view rather than
//! stalling indefinitely on a hardware fault.

use neo_consensus::error::ConsensusError;
use thiserror::Error;

/// Errors that can occur during HSM initialization and signing.
#[derive(Debug, Error)]
pub enum HsmError {
    /// The PKCS#11 library could not be loaded or initialized.
    #[error("HSM init error: {0}")]
    Init(String),

    /// Login to the HSM token failed.
    #[error("HSM login failed: {0}")]
    Login(String),

    /// The requested key was not found on the token.
    #[error("HSM key not found: label={label}")]
    KeyNotFound {
        /// CKA_LABEL value that was searched for.
        label: String,
    },

    /// The key's public point could not be read or decoded.
    #[error("HSM public key error: {0}")]
    PublicKey(String),

    /// A signing operation failed.
    #[error("HSM sign error: {0}")]
    Sign(String),

    /// The DER-to-raw-r||s conversion failed (GCP path).
    #[error("HSM signature decode error: {0}")]
    SigDecode(String),

    /// Low-s normalization failed.
    #[error("HSM low-s normalize error: {0}")]
    Normalize(String),

    /// The PKCS#11 feature is not compiled in.
    #[error("HSM feature not enabled: compile with --features pkcs11")]
    FeatureNotEnabled,

    /// The native Azure feature is not compiled in.
    #[error("HSM azure feature not enabled: compile with --features azure")]
    AzureFeatureNotEnabled,

    /// The native GCP feature is not compiled in.
    #[error("HSM gcp feature not enabled: compile with --features gcp")]
    GcpFeatureNotEnabled,

    /// The worker thread is gone (e.g. panic or HSM disconnected).
    #[error("HSM worker disconnected")]
    Disconnected,

    /// A sign request timed out waiting for the HSM worker.
    #[error("HSM sign timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// The signed-bytes length returned by the HSM was unexpected.
    #[error("HSM unexpected signature length: expected {expected}, got {got}")]
    UnexpectedSigLen {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },

    /// A generic underlying PKCS#11 error (string-encoded to avoid the cryptoki
    /// error type leaking into the public API when the feature is disabled).
    #[cfg(feature = "pkcs11")]
    #[error("PKCS#11 error: {0}")]
    Pkcs11(#[from] cryptoki::error::Error),

    /// An HTTP/REST error from the Azure native path.
    #[cfg(feature = "azure")]
    #[error("Azure REST error: {0}")]
    AzureHttp(#[from] reqwest::Error),
}

/// Result alias for HSM operations.
pub type HsmResult<T> = Result<T, HsmError>;

impl From<HsmError> for ConsensusError {
    fn from(e: HsmError) -> Self {
        // All HSM errors become a state_error so dBFT sees a transient fault
        // and triggers change-view, rather than crashing the consensus driver.
        ConsensusError::state_error(format!("hsm: {e}"))
    }
}

impl From<HsmError> for neo_error::CoreError {
    fn from(err: HsmError) -> Self {
        neo_error::CoreError::Cryptographic {
            message: err.to_string(),
        }
    }
}

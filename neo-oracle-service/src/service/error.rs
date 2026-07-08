//! Typed oracle service errors.

use thiserror::Error;

/// Errors returned by oracle service operations.
#[derive(Debug, Error)]
pub enum OracleServiceError {
    /// Oracle service is not running or is disabled by configuration.
    #[error("oracle service disabled")]
    Disabled,
    /// The request was already completed and cached as finished.
    #[error("oracle request already finished")]
    RequestFinished,
    /// The requested oracle entry does not exist in native contract storage.
    #[error("oracle request not found")]
    RequestNotFound,
    /// The supplied public key is not designated as an oracle node.
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    /// The oracle response signature failed verification.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    /// Oracle public key bytes could not be parsed or validated.
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    /// The response transaction for the request could not be located.
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    /// Building the oracle response transaction failed.
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    /// Request processing failed with an implementation-specific reason.
    #[error("oracle processing error: {0}")]
    Processing(String),
    /// The same oracle request is already being processed.
    #[error("duplicate request")]
    DuplicateRequest,
    /// The oracle URL was rejected by the service security policy.
    #[error("URL blocked by security policy")]
    UrlBlocked,
}

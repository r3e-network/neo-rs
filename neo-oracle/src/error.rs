//! Error types for the Oracle service.

use thiserror::Error;

/// Errors raised while processing oracle requests and responses.
#[derive(Debug, Error)]
pub enum OracleServiceError {
    /// The service is disabled by configuration.
    #[error("oracle service disabled")]
    Disabled,
    /// The request was already responded to and retired.
    #[error("oracle request already finished")]
    RequestFinished,
    /// No active oracle request matches the given id.
    #[error("oracle request not found")]
    RequestNotFound,
    /// This node is not among the designated responders for the request.
    #[error("oracle not designated: {0}")]
    NotDesignated(String),
    /// A submitted signature failed verification.
    #[error("invalid signature: {0}")]
    InvalidSignature(String),
    /// The supplied oracle public key is malformed.
    #[error("invalid oracle public key")]
    InvalidOraclePublicKey,
    /// The request transaction is absent from the ledger.
    #[error("oracle request transaction not found")]
    RequestTransactionNotFound,
    /// Building the response transaction failed.
    #[error("oracle response build failed: {0}")]
    BuildFailed(String),
    /// An internal processing step failed.
    #[error("oracle processing error: {0}")]
    Processing(String),
    /// The same request was submitted twice within the dedup window.
    #[error("duplicate request")]
    DuplicateRequest,
    /// The request URL was rejected by the SSRF/whitelist policy.
    #[error("URL blocked by security policy")]
    UrlBlocked,
}

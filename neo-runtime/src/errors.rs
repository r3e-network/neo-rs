//! Service-layer error type shared by every reth-style service in [`neo_runtime`].
//!
//! `ServiceError` is the single vocabulary used by the `BlockExecutor`,
//! `MempoolService`, `NetworkService`, `ConsensusService`, `NeoEngine`, and
//! `BlockchainHandle` APIs. Lower-layer errors (storage, IO, crypto, …) are
//! lifted into the `Internal` arm by the concrete service implementation; the
//! trait-level API never exposes a foreign error type. This keeps the public
//! surface of the runtime crate free of cross-crate error leakage and matches
//! the reth `ProviderError` / `BlockExecutionError` style of "one error type
//! per layer, lifted into the trait error at the boundary".
//!
//! See module-level documentation in [`crate`] for the overall design.

use thiserror::Error;

/// Errors that any service in the runtime can surface to a caller.
///
/// `ServiceError` is intentionally narrow: the trait-level API of each
/// service uses this type for *all* failure modes. The richer per-subsystem
/// error types (storage errors, consensus errors, …) live in their respective
/// crates and are mapped into `ServiceError::Internal` at the service
/// boundary.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The targeted service is not running, the command channel is closed,
    /// or the underlying actor has been shut down.
    ///
    /// Callers can usually recover by recreating the [`crate::Node`] and
    /// re-issuing the request.
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Caller supplied a malformed block / transaction / payload.
    ///
    /// This is a *permanent* failure: retrying with the same input will fail
    /// in the same way. The caller is expected to surface the error to the
    /// end user.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The requested resource (block, transaction, peer, …) does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The requested operation is not allowed in the current state
    /// (e.g. importing a block whose parent is unknown, broadcasting while
    /// the network is still starting up).
    #[error("invalid state: {0}")]
    InvalidState(String),

    /// The operation timed out before it could complete.
    ///
    /// Usually backed by a `tokio::time::timeout` on the service call.
    #[error("operation timed out: {0}")]
    Timeout(String),

    /// Catch-all for everything that does not fit the categories above.
    ///
    /// Concrete service implementations should wrap foreign errors (storage,
    /// IO, cryptography, VM) into this arm rather than leaking them through
    /// the trait API.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ServiceError {
    /// Returns `true` for transient errors that may succeed if retried.
    ///
    /// The `ServiceUnavailable` and `Timeout` arms are considered
    /// retryable; the `InvalidInput` / `NotFound` / `InvalidState` arms are
    /// not.
    pub fn is_retryable(&self) -> bool {
        matches!(self, ServiceError::ServiceUnavailable(_) | ServiceError::Timeout(_))
    }

    /// Returns the [`ServiceError`] category as a stable lowercase string.
    ///
    /// Useful for metrics labels and structured logging.
    pub fn category(&self) -> &'static str {
        match self {
            ServiceError::ServiceUnavailable(_) => "service_unavailable",
            ServiceError::InvalidInput(_) => "invalid_input",
            ServiceError::NotFound(_) => "not_found",
            ServiceError::InvalidState(_) => "invalid_state",
            ServiceError::Timeout(_) => "timeout",
            ServiceError::Internal(_) => "internal",
        }
    }

    /// Construct a `ServiceError::ServiceUnavailable` from any string-like
    /// value. Mirrors the helper constructors on other Neo error types.
    pub fn unavailable<E: ToString>(err: E) -> Self {
        ServiceError::ServiceUnavailable(err.to_string())
    }

    /// Construct a `ServiceError::InvalidInput` from any string-like value.
    pub fn invalid_input<E: ToString>(err: E) -> Self {
        ServiceError::InvalidInput(err.to_string())
    }

    /// Construct a `ServiceError::NotFound` from any string-like value.
    pub fn not_found<E: ToString>(err: E) -> Self {
        ServiceError::NotFound(err.to_string())
    }

    /// Construct a `ServiceError::InvalidState` from any string-like value.
    pub fn invalid_state<E: ToString>(err: E) -> Self {
        ServiceError::InvalidState(err.to_string())
    }

    /// Construct a `ServiceError::Timeout` from any string-like value.
    pub fn timeout<E: ToString>(err: E) -> Self {
        ServiceError::Timeout(err.to_string())
    }

    /// Construct a `ServiceError::Internal` from any string-like value.
    pub fn internal<E: ToString>(err: E) -> Self {
        ServiceError::Internal(err.to_string())
    }
}

/// Result alias used by every service method in [`neo_runtime`].
pub type ServiceResult<T> = Result<T, ServiceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_classification() {
        assert!(ServiceError::unavailable("x").is_retryable());
        assert!(ServiceError::timeout("x").is_retryable());
        assert!(!ServiceError::invalid_input("x").is_retryable());
        assert!(!ServiceError::not_found("x").is_retryable());
        assert!(!ServiceError::invalid_state("x").is_retryable());
        assert!(!ServiceError::internal("x").is_retryable());
    }

    #[test]
    fn category_is_stable() {
        assert_eq!(ServiceError::unavailable("x").category(), "service_unavailable");
        assert_eq!(ServiceError::invalid_input("x").category(), "invalid_input");
        assert_eq!(ServiceError::not_found("x").category(), "not_found");
        assert_eq!(ServiceError::invalid_state("x").category(), "invalid_state");
        assert_eq!(ServiceError::timeout("x").category(), "timeout");
        assert_eq!(ServiceError::internal("x").category(), "internal");
    }

    #[test]
    fn display_includes_message() {
        let err = ServiceError::unavailable("blockchain");
        assert_eq!(err.to_string(), "service unavailable: blockchain");
    }
}

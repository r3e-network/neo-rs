//! Network-layer error vocabulary.
//!
//! The reth-style services in this crate return [`NetworkError`] from
//! any operation that has to cross an `await` boundary. The
//! vocabulary is intentionally small — the same `ServiceError` from
//! `neo_runtime` is reused by the [`crate::handle::NetworkHandle`]
//! for the trait-level API.

use thiserror::Error;

/// Cross-service network error vocabulary.
///
/// Distinct from [`neo_runtime::ServiceError`] (which is the
/// runtime-wide vocabulary used by every service trait object) so
/// network-specific failure modes have a clear home. The
/// `From<NetworkError> for ServiceError` impl below bridges the two.
#[derive(Debug, Error)]
pub enum NetworkError {
    /// A TCP listener could not be bound, accepted, or read.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A peer operation could not complete, for example because the
    /// per-connection service exited or a correlated range fetch expired.
    /// The latter fails only the assignment and does not imply that the
    /// underlying connection was closed.
    #[error("remote node {peer_id:?} is unavailable: {detail}")]
    RemoteUnavailable {
        /// Identifier of the remote node service.
        peer_id: String,
        /// Human-readable failure detail.
        detail: String,
    },

    /// The local node's command channel was closed while the request
    /// was in flight.
    #[error("local node service is shutting down")]
    LocalShuttingDown,

    /// The local node's command channel is currently full; callers may retry.
    #[error("local node command channel is full")]
    ChannelFull,

    /// The local node has not been `start()`-ed yet.
    #[error("local node service has not been started")]
    NotStarted,

    /// Catch-all for protocol-level errors that don't fit the other
    /// variants.
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl From<NetworkError> for neo_runtime::ServiceError {
    fn from(err: NetworkError) -> Self {
        match err {
            NetworkError::Io(e) => neo_runtime::ServiceError::Internal(e.to_string()),
            NetworkError::RemoteUnavailable { detail, .. } => {
                neo_runtime::ServiceError::ServiceUnavailable(format!("remote: {detail}"))
            }
            NetworkError::LocalShuttingDown => neo_runtime::ServiceError::ServiceUnavailable(
                "local node shutting down".to_string(),
            ),
            NetworkError::ChannelFull => neo_runtime::ServiceError::ServiceUnavailable(
                "local node command channel is full".to_string(),
            ),
            NetworkError::NotStarted => {
                neo_runtime::ServiceError::InvalidState("local node not started".to_string())
            }
            NetworkError::Protocol(msg) => {
                neo_runtime::ServiceError::Internal(format!("protocol: {msg}"))
            }
        }
    }
}

/// Result alias for network-layer operations.
pub type NetworkResult<T> = Result<T, NetworkError>;

neo_error::impl_error_from_struct!(neo_error::CoreError, NetworkError => Network);

//! Network error types for P2P operations.
//!
//! NOTE: This type is named `NetworkError` for historical reasons. It represents
//! **P2P protocol-level** errors (protocol violations, invalid messages, connection
//! failures). It is distinct from `neo_network::NetworkError`, which represents
//! **network service-level** errors (shutdown, remote unavailable). The alias
//! `P2pError` is preferred for new code to avoid the name collision.

use std::net::SocketAddr;
use thiserror::Error;

/// P2P protocol-level network errors.
///
/// Also re-exported as `P2pError` — prefer that name in new code to avoid
/// collision with `neo_network::NetworkError`.
#[derive(Debug, Clone, Error)]
pub enum NetworkError {
    /// Protocol violation by a peer.
    #[error("Protocol violation from {peer}: {violation}")]
    ProtocolViolation {
        /// The peer that violated the protocol.
        peer: SocketAddr,
        /// Description of the violation.
        violation: String,
    },

    /// Invalid message format.
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Connection error.
    #[error("Connection error: {0}")]
    ConnectionError(String),

    /// Timeout error.
    #[error("Network timeout")]
    Timeout,

    /// Other network error.
    #[error("Network error: {0}")]
    Other(String),
}

/// Result type for network operations.
pub type NetworkResult<T> = Result<T, NetworkError>;

/// Preferred alias for [`NetworkError`] — avoids collision with
/// `neo_network::NetworkError`.
pub type P2pError = NetworkError;

/// Preferred alias for [`NetworkResult`] — avoids collision with
/// `neo_network::NetworkResult`.
pub type P2pResult<T> = NetworkResult<T>;

impl From<std::io::Error> for NetworkError {
    fn from(error: std::io::Error) -> Self {
        Self::ConnectionError(error.to_string())
    }
}

impl NetworkError {
    /// Returns true when the error represents a timeout condition.
    pub fn is_timeout(&self) -> bool {
        matches!(self, NetworkError::Timeout)
    }
}

#[cfg(test)]
#[path = "../tests/errors/network_error.rs"]
mod tests;

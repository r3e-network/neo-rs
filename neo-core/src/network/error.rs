//! Network error types for P2P operations.

use std::net::SocketAddr;
use thiserror::Error;

/// Network-related errors.
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

impl NetworkError {
    /// Returns true when the error represents a timeout condition.
    pub fn is_timeout(&self) -> bool {
        matches!(self, NetworkError::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::NetworkError;

    #[test]
    fn timeout_check_matches_variant() {
        assert!(NetworkError::Timeout.is_timeout());
        assert!(!NetworkError::ConnectionError("x".into()).is_timeout());
        assert!(!NetworkError::InvalidMessage("x".into()).is_timeout());
    }
}

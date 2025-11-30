//! Network error types for P2P operations.

use std::fmt;
use std::net::SocketAddr;

/// Network-related errors.
#[derive(Debug, Clone)]
pub enum NetworkError {
    /// Protocol violation by a peer.
    ProtocolViolation {
        /// The peer that violated the protocol.
        peer: SocketAddr,
        /// Description of the violation.
        violation: String,
    },

    /// Invalid message format.
    InvalidMessage(String),

    /// Connection error.
    ConnectionError(String),

    /// Timeout error.
    Timeout,

    /// Other network error.
    Other(String),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProtocolViolation { peer, violation } => {
                write!(f, "Protocol violation from {}: {}", peer, violation)
            }
            Self::InvalidMessage(msg) => write!(f, "Invalid message: {}", msg),
            Self::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            Self::Timeout => write!(f, "Network timeout"),
            Self::Other(msg) => write!(f, "Network error: {}", msg),
        }
    }
}

impl std::error::Error for NetworkError {}

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

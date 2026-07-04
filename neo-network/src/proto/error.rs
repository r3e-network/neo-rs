//! Error types for P2P operations.

use std::net::SocketAddr;
use thiserror::Error;

/// Errors that can occur during P2P operations.
#[derive(Error, Debug, Clone)]
pub enum P2PError {
    /// Connection failed.
    #[error("Connection failed: {message}")]
    ConnectionFailed {
        /// Error message.
        message: String,
    },

    /// Peer disconnected.
    #[error("Peer disconnected: {peer_id}")]
    PeerDisconnected {
        /// Peer identifier.
        peer_id: String,
    },

    /// Invalid message.
    #[error("Invalid message: {message}")]
    InvalidMessage {
        /// Error message.
        message: String,
    },

    /// Protocol error.
    #[error("Protocol error: {message}")]
    ProtocolError {
        /// Error message.
        message: String,
    },

    /// Protocol violation by a peer.
    #[error("Protocol violation from {peer}: {violation}")]
    ProtocolViolation {
        /// The peer that violated the protocol.
        peer: SocketAddr,
        /// Description of the violation.
        violation: String,
    },

    /// Timeout.
    #[error("Timeout: {operation}")]
    Timeout {
        /// Operation that timed out.
        operation: String,
    },

    /// IO error.
    #[error("IO error: {message}")]
    Io {
        /// Error message.
        message: String,
    },

    /// Other network error.
    #[error("Network error: {message}")]
    Other {
        /// Error message.
        message: String,
    },
}

impl P2PError {
    /// Create a connection failed error.
    pub fn connection_failed<S: Into<String>>(message: S) -> Self {
        Self::ConnectionFailed {
            message: message.into(),
        }
    }

    /// Create an invalid message error.
    pub fn invalid_message<S: Into<String>>(message: S) -> Self {
        Self::InvalidMessage {
            message: message.into(),
        }
    }

    /// Create a protocol error.
    pub fn protocol_error<S: Into<String>>(message: S) -> Self {
        Self::ProtocolError {
            message: message.into(),
        }
    }

    /// Create a protocol violation error.
    pub fn protocol_violation<S: Into<String>>(peer: SocketAddr, violation: S) -> Self {
        Self::ProtocolViolation {
            peer,
            violation: violation.into(),
        }
    }

    /// Create a timeout error.
    pub fn timeout<S: Into<String>>(operation: S) -> Self {
        Self::Timeout {
            operation: operation.into(),
        }
    }

    /// Create an IO error.
    pub fn io<S: Into<String>>(message: S) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    /// Returns true when the error represents a timeout condition.
    pub fn is_timeout(&self) -> bool {
        matches!(self, P2PError::Timeout { .. })
    }
}

impl From<std::io::Error> for P2PError {
    fn from(error: std::io::Error) -> Self {
        Self::io(error.to_string())
    }
}

/// Result type for P2P operations.
pub type P2PResult<T> = std::result::Result<T, P2PError>;

impl From<P2PError> for neo_error::CoreError {
    fn from(err: P2PError) -> Self {
        neo_error::CoreError::Network {
            message: err.to_string(),
        }
    }
}

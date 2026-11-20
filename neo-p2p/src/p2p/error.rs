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

    /// Connection error.
    #[error("Connection error: {0}")]
    ConnectionError(String),

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

/// Failure to compute a consensus payload hash.
///
/// The payload hash helpers report `IoError`, which neither clones nor compares
/// and belongs to the serialization layer. P2P block outcomes need both, so the
/// failure is captured here as its rendered message and `neo-p2p` keeps no
/// dependency on the host runtime error vocabulary.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
#[error("payload hash computation failed: {message}")]
pub struct HashComputationError {
    message: String,
}

impl HashComputationError {
    /// Creates the error from a rendered message.
    pub fn new<S: Into<String>>(message: S) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the rendered failure message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<neo_io_crate::IoError> for HashComputationError {
    fn from(error: neo_io_crate::IoError) -> Self {
        Self::new(error.to_string())
    }
}

impl From<neo_io_crate::IoError> for P2PError {
    fn from(error: neo_io_crate::IoError) -> Self {
        Self::io(error.to_string())
    }
}

impl From<neo_primitives::NetworkError> for P2PError {
    fn from(error: neo_primitives::NetworkError) -> Self {
        match error {
            neo_primitives::NetworkError::ProtocolViolation { peer, violation } => {
                Self::ProtocolViolation { peer, violation }
            }
            neo_primitives::NetworkError::InvalidMessage(msg) => {
                Self::InvalidMessage { message: msg }
            }
            neo_primitives::NetworkError::ConnectionError(msg) => Self::ConnectionError(msg),
            neo_primitives::NetworkError::Timeout => Self::Timeout {
                operation: "network operation".to_string(),
            },
            neo_primitives::NetworkError::Other(msg) => Self::Other { message: msg },
        }
    }
}

impl From<crate::p2p::protocol_message::P2PMessageError> for P2PError {
    fn from(error: crate::p2p::protocol_message::P2PMessageError) -> Self {
        match error {
            crate::p2p::protocol_message::P2PMessageError::InvalidMessage(msg) => {
                Self::InvalidMessage { message: msg }
            }
            crate::p2p::protocol_message::P2PMessageError::Io(msg) => Self::Io { message: msg },
        }
    }
}

/// Result type for P2P operations.
pub type P2PResult<T> = std::result::Result<T, P2PError>;

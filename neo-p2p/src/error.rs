//! Error types for P2P operations.

use thiserror::Error;

/// Errors that can occur during P2P operations.
#[derive(Error, Debug)]
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

    /// Timeout.
    #[error("Timeout: {operation}")]
    Timeout {
        /// Operation that timed out.
        operation: String,
    },

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
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
}

/// Result type for P2P operations.
pub type P2PResult<T> = std::result::Result<T, P2PError>;

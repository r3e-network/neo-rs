//! Errors that can occur during peer message sending.

use thiserror::Error;

/// Errors that can occur during peer communication.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Peer not found.
    #[error("peer not found: {id}")]
    PeerNotFound {
        /// Peer ID that was not found.
        id: u64,
    },

    /// Peer is disconnected.
    #[error("peer disconnected: {id}")]
    Disconnected {
        /// Peer ID that is disconnected.
        id: u64,
    },

    /// Send queue is full.
    #[error("send queue full for peer {id}")]
    QueueFull {
        /// Peer ID whose queue is full.
        id: u64,
    },

    /// Serialization error.
    #[error("message serialization failed: {message}")]
    SerializationFailed {
        /// Detailed error message.
        message: String,
    },
}

impl SendError {
    /// Create a peer not found error.
    #[must_use]
    pub const fn peer_not_found(id: u64) -> Self {
        Self::PeerNotFound { id }
    }

    /// Create a disconnected error.
    #[must_use]
    pub const fn disconnected(id: u64) -> Self {
        Self::Disconnected { id }
    }

    /// Create a queue full error.
    #[must_use]
    pub const fn queue_full(id: u64) -> Self {
        Self::QueueFull { id }
    }

    /// Create a serialization failed error.
    pub fn serialization_failed<S: Into<String>>(message: S) -> Self {
        Self::SerializationFailed {
            message: message.into(),
        }
    }
}

/// Result type for send operations.
pub type SendResult<T> = Result<T, SendError>;

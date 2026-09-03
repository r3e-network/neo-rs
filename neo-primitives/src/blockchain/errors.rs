use crate::UInt256;
use thiserror::Error;

// ============ Error Types ============

/// Errors that can occur during block/transaction relay.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum RelayError {
    /// Block validation failed.
    #[error("block validation failed: {message}")]
    ValidationFailed {
        /// Detailed error message.
        message: String,
    },

    /// Block already exists in the blockchain.
    #[error("block already exists: {hash}")]
    AlreadyExists {
        /// Hash of the existing block.
        hash: String,
    },

    /// Transaction validation failed.
    #[error("transaction invalid: {message}")]
    TransactionInvalid {
        /// Detailed error message.
        message: String,
    },

    /// Memory pool is full.
    #[error("memory pool full: size={current}, max={max}")]
    MempoolFull {
        /// Current mempool size.
        current: usize,
        /// Maximum mempool size.
        max: usize,
    },

    /// Block height is invalid.
    #[error("invalid block height: expected={expected}, got={got}")]
    InvalidHeight {
        /// Expected block height.
        expected: u32,
        /// Actual block height.
        got: u32,
    },
}

impl RelayError {
    /// Create a validation failed error.
    pub fn validation_failed<S: Into<String>>(message: S) -> Self {
        Self::ValidationFailed {
            message: message.into(),
        }
    }

    /// Create an already exists error.
    #[must_use]
    pub fn already_exists(hash: &UInt256) -> Self {
        Self::AlreadyExists {
            hash: format!("{hash:?}"),
        }
    }

    /// Create a transaction invalid error.
    pub fn transaction_invalid<S: Into<String>>(message: S) -> Self {
        Self::TransactionInvalid {
            message: message.into(),
        }
    }

    /// Create a mempool full error.
    #[must_use]
    pub const fn mempool_full(current: usize, max: usize) -> Self {
        Self::MempoolFull { current, max }
    }

    /// Create an invalid height error.
    #[must_use]
    pub const fn invalid_height(expected: u32, got: u32) -> Self {
        Self::InvalidHeight { expected, got }
    }
}

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

/// Result type for relay operations.
pub type RelayResult<T> = Result<T, RelayError>;

/// Result type for send operations.
pub type SendResult<T> = Result<T, SendError>;

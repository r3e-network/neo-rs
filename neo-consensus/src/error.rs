//! Error types for consensus operations.
//!
//! This module defines the error types for dBFT consensus failures.
//!
//! ## Error Categories
//!
//! | Error | Description |
//! |-------|-------------|
//! | `InvalidView` | View number mismatch |
//! | `InvalidProposal` | Invalid block proposal |
//! | `SignatureVerificationFailed` | Signature check failed |
//! | `HashMismatch` | Block hash mismatch |
//! | `Timeout` | Consensus phase timeout |
//! | `InsufficientSignatures` | Not enough signatures for block |
//!
//! ## Example
//!
//! ```rust
//! use neo_consensus::error::ConsensusError;
//!
//! // Create an invalid proposal error
//! let err = ConsensusError::invalid_proposal("invalid timestamp");
//!
//! // Convert to string
//! assert!(err.to_string().contains("Invalid block proposal"));
//! ```

use neo_primitives::UInt256;
use thiserror::Error;

/// Errors that can occur during consensus operations.
#[derive(Error, Debug)]
pub enum ConsensusError {
    /// Invalid view number.
    #[error("Invalid view: expected {expected}, got {actual}")]
    InvalidView {
        /// Expected view number.
        expected: u32,
        /// Actual view number.
        actual: u32,
    },

    /// Invalid view number for change view.
    #[error("Invalid view number: current {current}, requested {requested}")]
    InvalidViewNumber {
        /// Current view number.
        current: u8,
        /// Requested view number.
        requested: u8,
    },

    /// Invalid block proposal.
    #[error("Invalid block proposal: {message}")]
    InvalidProposal {
        /// Error message.
        message: String,
    },

    /// Signature verification failed.
    #[error("Signature verification failed: {message}")]
    SignatureVerificationFailed {
        /// Error message.
        message: String,
    },

    /// Invalid signature length.
    #[error("Invalid signature length: expected {expected}, got {got}")]
    InvalidSignatureLength {
        /// Expected length.
        expected: usize,
        /// Actual length.
        got: usize,
    },

    /// Hash mismatch.
    #[error("Hash mismatch: expected {expected}, got {got}")]
    HashMismatch {
        /// Expected hash.
        expected: UInt256,
        /// Actual hash.
        got: UInt256,
    },

    /// Invalid primary.
    #[error("Invalid primary: expected {expected}, got {got}")]
    InvalidPrimary {
        /// Expected primary index.
        expected: u8,
        /// Actual primary index.
        got: u8,
    },

    /// Invalid validator index.
    #[error("Invalid validator index: {0}")]
    InvalidValidatorIndex(u8),

    /// Duplicate validator.
    #[error("Duplicate validator: {0}")]
    DuplicateValidator(u8),

    /// Not a validator.
    #[error("Not a validator")]
    NotValidator,

    /// Timeout.
    #[error("Consensus timeout: {phase}")]
    Timeout {
        /// Consensus phase that timed out.
        phase: String,
    },

    /// State error.
    #[error("State error: {message}")]
    StateError {
        /// Error message.
        message: String,
    },

    /// Message from wrong block.
    #[error("Message from wrong block: expected {expected}, got {got}")]
    WrongBlock {
        /// Expected block index.
        expected: u32,
        /// Actual block index.
        got: u32,
    },

    /// Message from wrong view.
    #[error("Message from wrong view: expected {expected}, got {got}")]
    WrongView {
        /// Expected view number.
        expected: u8,
        /// Actual view number.
        got: u8,
    },

    /// Already received message.
    #[error("Already received message from validator {0}")]
    AlreadyReceived(u8),

    /// Channel send error (stringly-typed fallback).
    #[error("Channel send error: {0}")]
    ChannelError(String),

    /// Channel send error with preserved source.
    #[error("Channel send error")]
    ChannelSendError(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// Persistence error.
    #[error("Persistence error: {0}")]
    PersistenceError(String),

    /// IO error.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error (stringly-typed fallback).
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Bincode serialization/deserialization error.
    #[error("Serialization error")]
    BincodeError(#[from] bincode::Error),

    /// Insufficient signatures for block assembly.
    #[error("Insufficient signatures: required {required}, got {got}")]
    InsufficientSignatures {
        /// Required number of signatures.
        required: usize,
        /// Actual number of signatures.
        got: usize,
    },
}

impl ConsensusError {
    /// Create an invalid proposal error.
    pub fn invalid_proposal<S: Into<String>>(message: S) -> Self {
        Self::InvalidProposal {
            message: message.into(),
        }
    }

    /// Create a signature verification failed error.
    pub fn signature_failed<S: Into<String>>(message: S) -> Self {
        Self::SignatureVerificationFailed {
            message: message.into(),
        }
    }

    /// Create a state error.
    pub fn state_error<S: Into<String>>(message: S) -> Self {
        Self::StateError {
            message: message.into(),
        }
    }
}

/// Result type for consensus operations.
pub type ConsensusResult<T> = std::result::Result<T, ConsensusError>;

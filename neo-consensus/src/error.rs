//! Error types for consensus operations.

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

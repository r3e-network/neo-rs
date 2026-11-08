//! Error types for fast path execution.

use std::fmt;

/// Result type for fast path operations.
pub type FastPathResult<T> = Result<T, FastPathError>;

/// Errors that can occur during fast path execution.
#[derive(Debug, Clone)]
pub enum FastPathError {
    /// Script pattern doesn't match expected format
    InvalidPattern(String),

    /// Insufficient balance for transfer
    InsufficientBalance,

    /// Storage operation failed
    StorageError(String),

    /// Failed to parse script parameters
    ParseError(String),

    /// Contract not found
    ContractNotFound,

    /// Invalid script length
    InvalidScriptLength(usize),

    /// Fast path not applicable - fallback to full VM required
    NotApplicable(String),
}

impl fmt::Display for FastPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPattern(msg) => write!(f, "Invalid script pattern: {}", msg),
            Self::InsufficientBalance => write!(f, "Insufficient balance for transfer"),
            Self::StorageError(msg) => write!(f, "Storage error: {}", msg),
            Self::ParseError(msg) => write!(f, "Parse error: {}", msg),
            Self::ContractNotFound => write!(f, "Contract not found"),
            Self::InvalidScriptLength(len) => write!(f, "Invalid script length: {}", len),
            Self::NotApplicable(msg) => write!(f, "Fast path not applicable: {}", msg),
        }
    }
}

impl std::error::Error for FastPathError {}

// Copyright (C) 2015-2025 The Neo Project.
//
// error.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! Error types for the neo-state crate.

use neo_primitives::UInt160;
use thiserror::Error;

/// Result type for state operations.
pub type StateResult<T> = Result<T, StateError>;

/// Errors that can occur during state operations.
#[derive(Debug, Error)]
pub enum StateError {
    /// Account not found in state.
    #[error("account not found: {0}")]
    AccountNotFound(UInt160),

    /// Contract not found in state.
    #[error("contract not found: {0}")]
    ContractNotFound(UInt160),

    /// Storage key not found.
    #[error("storage key not found")]
    StorageKeyNotFound,

    /// Snapshot already committed.
    #[error("snapshot already committed")]
    SnapshotAlreadyCommitted,

    /// Snapshot already rolled back.
    #[error("snapshot already rolled back")]
    SnapshotAlreadyRolledBack,

    /// Invalid snapshot state.
    #[error("invalid snapshot state: {0}")]
    InvalidSnapshotState(String),

    /// Storage error.
    #[error("storage error: {0}")]
    Storage(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// State root mismatch.
    #[error("state root mismatch: expected {expected}, got {actual}")]
    StateRootMismatch {
        expected: String,
        actual: String,
    },

    /// Invalid state transition.
    #[error("invalid state transition: {0}")]
    InvalidStateTransition(String),

    /// Concurrent modification detected.
    #[error("concurrent modification detected")]
    ConcurrentModification,

    /// Maximum depth exceeded.
    #[error("maximum snapshot depth exceeded: {0}")]
    MaxDepthExceeded(usize),
}

impl From<neo_storage::StorageError> for StateError {
    fn from(err: neo_storage::StorageError) -> Self {
        StateError::Storage(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StateError::AccountNotFound(UInt160::default());
        assert!(err.to_string().contains("account not found"));

        let err = StateError::ContractNotFound(UInt160::default());
        assert!(err.to_string().contains("contract not found"));

        let err = StateError::StorageKeyNotFound;
        assert_eq!(err.to_string(), "storage key not found");

        let err = StateError::SnapshotAlreadyCommitted;
        assert_eq!(err.to_string(), "snapshot already committed");
    }

    #[test]
    fn test_state_root_mismatch_error() {
        let err = StateError::StateRootMismatch {
            expected: "abc".to_string(),
            actual: "def".to_string(),
        };
        assert!(err.to_string().contains("expected abc"));
        assert!(err.to_string().contains("got def"));
    }
}

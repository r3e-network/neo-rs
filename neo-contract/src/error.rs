//! Error types for contract operations.

use thiserror::Error;

/// Errors that can occur during contract execution.
#[derive(Error, Debug, Clone)]
pub enum ContractError {
    /// Contract not found.
    #[error("Contract not found: {hash}")]
    ContractNotFound {
        /// Contract hash.
        hash: String,
    },

    /// Invalid contract manifest.
    #[error("Invalid manifest: {message}")]
    InvalidManifest {
        /// Error message.
        message: String,
    },

    /// Contract execution failed.
    #[error("Execution failed: {message}")]
    ExecutionFailed {
        /// Error message.
        message: String,
    },

    /// Insufficient gas.
    #[error("Insufficient gas: required {required}, available {available}")]
    InsufficientGas {
        /// Required gas amount.
        required: i64,
        /// Available gas amount.
        available: i64,
    },

    /// Permission denied.
    #[error("Permission denied: {message}")]
    PermissionDenied {
        /// Error message.
        message: String,
    },

    /// Invalid operation.
    #[error("Invalid operation: {message}")]
    InvalidOperation {
        /// Error message.
        message: String,
    },

    /// Native contract error.
    #[error("Native contract error: {message}")]
    NativeContract {
        /// Error message.
        message: String,
    },

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] neo_storage::StorageError),
}

impl ContractError {
    /// Create a contract not found error.
    pub fn contract_not_found<S: Into<String>>(hash: S) -> Self {
        Self::ContractNotFound { hash: hash.into() }
    }

    /// Create an invalid manifest error.
    pub fn invalid_manifest<S: Into<String>>(message: S) -> Self {
        Self::InvalidManifest {
            message: message.into(),
        }
    }

    /// Create an execution failed error.
    pub fn execution_failed<S: Into<String>>(message: S) -> Self {
        Self::ExecutionFailed {
            message: message.into(),
        }
    }

    /// Create a native contract error.
    pub fn native_contract<S: Into<String>>(message: S) -> Self {
        Self::NativeContract {
            message: message.into(),
        }
    }
}

/// Result type for contract operations.
pub type ContractResult<T> = std::result::Result<T, ContractError>;

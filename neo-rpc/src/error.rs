//! Error types for RPC operations.

use thiserror::Error;

/// Errors that can occur during RPC operations.
#[derive(Error, Debug)]
pub enum RpcError {
    /// Request failed.
    #[error("Request failed: {message}")]
    RequestFailed {
        /// Error message.
        message: String,
    },

    /// Invalid response.
    #[error("Invalid response: {message}")]
    InvalidResponse {
        /// Error message.
        message: String,
    },

    /// Method not found.
    #[error("Method not found: {method}")]
    MethodNotFound {
        /// Method name.
        method: String,
    },

    /// Invalid parameters.
    #[error("Invalid parameters: {message}")]
    InvalidParams {
        /// Error message.
        message: String,
    },

    /// Internal error.
    #[error("Internal error: {message}")]
    InternalError {
        /// Error message.
        message: String,
    },

    /// Serialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl RpcError {
    /// Create a request failed error.
    pub fn request_failed<S: Into<String>>(message: S) -> Self {
        Self::RequestFailed {
            message: message.into(),
        }
    }

    /// Create an invalid response error.
    pub fn invalid_response<S: Into<String>>(message: S) -> Self {
        Self::InvalidResponse {
            message: message.into(),
        }
    }

    /// Create an invalid params error.
    pub fn invalid_params<S: Into<String>>(message: S) -> Self {
        Self::InvalidParams {
            message: message.into(),
        }
    }
}

/// Result type for RPC operations.
pub type RpcResult<T> = std::result::Result<T, RpcError>;

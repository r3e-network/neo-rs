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

    /// UTF-8 conversion error.
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    /// JSON-RPC protocol error from the client transport layer.
    #[cfg(feature = "client")]
    #[error("{0}")]
    ClientRpc(#[from] crate::client::ClientRpcError),

    /// Base64 decoding error.
    #[cfg(feature = "client")]
    #[error("Base64 error: {0}")]
    Base64(#[from] base64::DecodeError),

    /// HTTP client error.
    #[cfg(feature = "client")]
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Invalid header value.
    #[cfg(feature = "client")]
    #[error("Invalid header: {0}")]
    InvalidHeader(#[from] reqwest::header::InvalidHeaderValue),

    /// VM execution error.
    #[cfg(feature = "client")]
    #[error("VM error: {0}")]
    Vm(#[from] neo_vm::VmError),

    /// Core domain error.
    #[cfg(feature = "client")]
    #[error("Core error: {0}")]
    Core(#[from] neo_core::CoreError),

    /// Generic error for domain-specific validation failures.
    #[error("{0}")]
    Other(String),
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

impl From<&str> for RpcError {
    fn from(s: &str) -> Self {
        Self::Other(s.to_string())
    }
}

impl From<String> for RpcError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

/// Result type for RPC operations.
pub type RpcResult<T> = std::result::Result<T, RpcError>;

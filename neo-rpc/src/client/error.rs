//! RPC error types

use std::fmt::{self, Display, Formatter};

/// Shared JSON-RPC exception representation containing the error code,
/// message, and optional data payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRpcError {
    code: i32,
    message: String,
    data: Option<String>,
}

impl ClientRpcError {
    /// Creates a new exception without additional data.
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Creates a new exception carrying an additional data payload.
    pub fn with_data(code: i32, message: impl Into<String>, data: impl Into<String>) -> Self {
        Self::from_parts(code, message, Some(data.into()))
    }

    /// Creates an exception from explicit parts, trimming empty data payloads.
    pub fn from_parts(code: i32, message: impl Into<String>, data: Option<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: data.and_then(|value| {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            }),
        }
    }

    /// Error code (matches JSON-RPC `code` field).
    #[must_use] 
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Human-readable message (matches JSON-RPC `message` field).
    #[must_use] 
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Optional JSON-RPC `data` payload if provided.
    #[must_use] 
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }
}

impl Display for ClientRpcError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.data {
            Some(data) => write!(f, "{} - {}", self.message, data),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for ClientRpcError {}

/// Backwards-compatible alias for the historical client error type name.
pub type RpcException = ClientRpcError;

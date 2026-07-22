//! # neo-rpc::server::rpc_exception
//!
//! Exception-style RPC error wrappers used by handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - [`RpcException`]: Handler error carrying the JSON-RPC code, message, and
//!   optional data payload.

use std::fmt::{self, Display, Formatter};

use super::rpc_error::RpcError;

/// JSON-RPC handler exception containing the error code, message, and optional data payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcException {
    code: i32,
    message: String,
    data: Option<String>,
}

impl RpcException {
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

    /// Returns the JSON-RPC error code.
    pub fn code(&self) -> i32 {
        self.code
    }

    /// Returns the human-readable JSON-RPC error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the optional JSON-RPC data payload.
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }
}

impl Display for RpcException {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.data {
            Some(data) => write!(f, "{} - {}", self.message, data),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for RpcException {}

impl From<RpcError> for RpcException {
    fn from(error: RpcError) -> Self {
        Self::from_parts(
            error.code(),
            error.message().to_string(),
            error.data().map(std::string::ToString::to_string),
        )
    }
}

impl From<RpcException> for RpcError {
    fn from(err: RpcException) -> Self {
        Self::new(
            err.code(),
            err.message().to_string(),
            err.data().map(std::string::ToString::to_string),
        )
    }
}

#[cfg(test)]
#[path = "../../tests/server/core/rpc_exception.rs"]
mod tests;

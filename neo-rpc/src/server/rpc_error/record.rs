//! JSON-RPC error record, formatting, and Neo JSON projection.

use neo_serialization::json::{JObject, JToken};
use std::fmt::{self, Display};

/// Represents a JSON-RPC error returned by the RPC server (matches the C#
/// `RpcError` class semantics).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcError {
    code: i32,
    message: String,
    data: Option<String>,
}

impl RpcError {
    /// Creates a new `RpcError` instance.
    pub fn new(code: i32, message: impl Into<String>, data: Option<String>) -> Self {
        let message = message.into();
        let data = data.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        Self {
            code,
            message,
            data,
        }
    }

    /// Returns the JSON-RPC error code.
    #[must_use]
    pub const fn code(&self) -> i32 {
        self.code
    }

    /// Returns the human readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns any additional error data when available.
    #[must_use]
    pub fn data(&self) -> Option<&str> {
        self.data.as_deref()
    }

    /// Creates a copy of the error carrying an additional data payload.
    pub fn with_data(&self, data: impl Into<String>) -> Self {
        Self {
            code: self.code,
            message: self.message.clone(),
            data: {
                let value = data.into();
                if value.trim().is_empty() {
                    None
                } else {
                    Some(value)
                }
            },
        }
    }

    /// Returns the formatted error message used for exceptions/logging.
    #[must_use]
    pub fn error_message(&self) -> String {
        match &self.data {
            Some(data) => format!("{} - {}", self.message, data),
            None => self.message.clone(),
        }
    }

    /// Serialises the error into a Neo JSON token (matches C# `ToJson`).
    #[must_use]
    pub fn to_json(&self) -> JToken {
        let mut obj = JObject::new();
        obj.set(
            "code".to_string(),
            Some(JToken::Number(f64::from(self.code))),
        );
        obj.set(
            "message".to_string(),
            Some(JToken::String(self.error_message())),
        );
        if let Some(data) = &self.data {
            obj.set("data".to_string(), Some(JToken::String(data.clone())));
        }
        JToken::Object(obj)
    }
}

impl Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.data {
            Some(data) => write!(f, "{} ({}) - {}", self.message, self.code, data),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

impl std::error::Error for RpcError {}

impl From<RpcError> for JToken {
    fn from(error: RpcError) -> Self {
        error.to_json()
    }
}

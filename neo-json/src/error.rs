use std::fmt;
use std::string::FromUtf8Error;

use thiserror::Error;

/// Errors returned by the Neo JSON module.
#[derive(Debug, Error)]
pub enum JsonError {
    #[error("index out of range: {0}")]
    IndexOutOfRange(usize),
    #[error("operation not supported: {0}")]
    NotSupported(&'static str),
    #[error("invalid cast: {0}")]
    InvalidCast(&'static str),
    #[error("overflow: {0}")]
    Overflow(&'static str),
    #[error("duplicate key: {0}")]
    DuplicateKey(String),
    #[error("format error: {0}")]
    Format(String),
    #[error(transparent)]
    Utf8(#[from] FromUtf8Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}

impl JsonError {
    pub fn format(message: impl Into<String>) -> Self {
        Self::Format(message.into())
    }

    pub fn duplicate_key(key: impl Into<String>) -> Self {
        Self::DuplicateKey(key.into())
    }
}

impl From<JsonError> for fmt::Error {
    fn from(_: JsonError) -> Self {
        Self
    }
}

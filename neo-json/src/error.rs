use std::fmt;
use std::string::FromUtf8Error;

use thiserror::Error;

/// Errors returned by the Neo JSON module.
#[derive(Debug, Error)]
pub enum JsonError {
    /// An index was out of the valid range for a JSON array.
    #[error("index out of range: {0}")]
    IndexOutOfRange(usize),
    /// The requested operation is not supported on this token type.
    #[error("operation not supported: {0}")]
    NotSupported(&'static str),
    /// A type cast between incompatible JSON token types failed.
    #[error("invalid cast: {0}")]
    InvalidCast(&'static str),
    /// A numeric value exceeded the representable range.
    #[error("overflow: {0}")]
    Overflow(&'static str),
    /// A duplicate key was encountered in a JSON object.
    #[error("duplicate key: {0}")]
    DuplicateKey(String),
    /// A general formatting or parsing error.
    #[error("format error: {0}")]
    Format(String),
    /// UTF-8 decoding failed.
    #[error(transparent)]
    Utf8(#[from] FromUtf8Error),
    /// An error from the underlying `serde_json` library.
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}

impl JsonError {
    /// Creates a [`Format`](Self::Format) error from a message.
    pub fn format(message: impl Into<String>) -> Self {
        Self::Format(message.into())
    }

    /// Creates a [`DuplicateKey`](Self::DuplicateKey) error from a key name.
    pub fn duplicate_key(key: impl Into<String>) -> Self {
        Self::DuplicateKey(key.into())
    }
}

impl From<JsonError> for fmt::Error {
    fn from(_: JsonError) -> Self {
        Self
    }
}

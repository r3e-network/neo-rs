use thiserror::Error;

#[derive(Error, Debug)]
pub enum JsonError {
    #[error("Parse error: {0}")]
    ParseError(#[from] serde_json::Error),
    #[error("Format error")]
    FormatError,
    #[error("Invalid cast")]
    InvalidCast,
    #[error("Index out of bounds")]
    IndexOutOfBounds,
    #[error("Key not found")]
    KeyNotFound,
    #[error("Invalid format")]
    InvalidFormat,
}

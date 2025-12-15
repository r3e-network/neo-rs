use neo_io::IoError;
use neo_primitives::PrimitiveError;
use thiserror::Error;

pub type MptResult<T> = Result<T, MptError>;

#[derive(Debug, Error)]
pub enum MptError {
    #[error("IO error: {0}")]
    Io(#[from] IoError),
    #[error("primitive error: {0}")]
    Primitive(#[from] PrimitiveError),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("invalid operation: {0}")]
    InvalidOperation(String),
    #[error("key error: {0}")]
    Key(String),
}

impl MptError {
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage(message.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }

    pub fn key(message: impl Into<String>) -> Self {
        Self::Key(message.into())
    }
}

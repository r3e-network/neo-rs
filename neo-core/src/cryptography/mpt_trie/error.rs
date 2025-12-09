use crate::error::CoreError;
use crate::neo_io::IoError;
use neo_primitives::PrimitiveError;
use thiserror::Error;

pub type MptResult<T> = Result<T, MptError>;

#[derive(Debug, Error)]
pub enum MptError {
    #[error("IO error: {0}")]
    Io(#[from] IoError),
    #[error("core error: {0}")]
    Core(#[from] CoreError),
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

impl From<PrimitiveError> for MptError {
    fn from(error: PrimitiveError) -> Self {
        CoreError::from(error).into()
    }
}

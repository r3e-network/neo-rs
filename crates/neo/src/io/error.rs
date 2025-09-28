use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IoError {
    #[error("Unexpected end of input")]
    UnexpectedEof,
    #[error("Invalid data: {0}")]
    InvalidData(String),
}

impl IoError {
    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidData(message.into())
    }

    pub fn end_of_stream(_position: usize, entity: &str) -> Self {
        Self::InvalidData(format!("Unexpected end of stream while reading {}", entity))
    }

    pub fn format_exception(operation: &str, reason: &str) -> Self {
        Self::InvalidData(format!("{}: {}", operation, reason))
    }
}

pub type IoResult<T> = Result<T, IoError>;

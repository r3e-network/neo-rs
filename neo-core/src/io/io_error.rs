use thiserror::Error;

#[derive(Debug, Error)]
pub enum IOError {
    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Read error: {0}")]
    ReadError(String),

    #[error("Write error: {0}")]
    WriteError(String),

    #[error("Unexpected end of file: {0}")]
    UnexpectedEof(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),

    #[error("Operation timed out")]
    Timeout,

    #[error("IO operation interrupted")]
    Interrupted,

    #[error("Other IO error: {0}")]
    Other(String),

    #[error(transparent)]
    Std(#[from] std::io::Error),
}

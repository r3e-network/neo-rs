use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("Key not found")]
    KeyNotFound,
    #[error("Invalid state")]
    InvalidState,
    #[error("Internal error: {0}")]
    InternalError(String),
}
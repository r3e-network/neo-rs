use thiserror::Error;

/// Result type for actor operations.
pub type AkkaResult<T> = Result<T, AkkaError>;

/// Errors returned by the Akka runtime implementation.
#[derive(Debug, Error)]
pub enum AkkaError {
    #[error("actor failure: {0}")]
    Actor(String),
    #[error("message send failed: {0}")]
    Send(String),
    #[error("ask timed out")]
    AskTimeout,
    #[error("system failure: {0}")]
    System(String),
}

impl AkkaError {
    pub fn actor<E: ToString>(err: E) -> Self {
        AkkaError::Actor(err.to_string())
    }

    pub fn send<E: ToString>(err: E) -> Self {
        AkkaError::Send(err.to_string())
    }

    pub fn system<E: ToString>(err: E) -> Self {
        AkkaError::System(err.to_string())
    }
}

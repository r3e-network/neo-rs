use thiserror::Error;

/// Result type for actor operations.
pub type ActorRuntimeResult<T> = Result<T, ActorRuntimeError>;

/// Errors returned by the actor runtime implementation.
#[derive(Debug, Error)]
pub enum ActorRuntimeError {
    #[error("actor failure: {0}")]
    Actor(String),
    #[error("message send failed: {0}")]
    Send(String),
    #[error("ask timed out")]
    AskTimeout,
    #[error("system failure: {0}")]
    System(String),
}

impl ActorRuntimeError {
    pub fn actor<E: ToString>(err: E) -> Self {
        Self::Actor(err.to_string())
    }

    pub fn send<E: ToString>(err: E) -> Self {
        Self::Send(err.to_string())
    }

    pub fn system<E: ToString>(err: E) -> Self {
        Self::System(err.to_string())
    }
}

/// Compatibility alias for older C#-ported call sites.
pub type AkkaError = ActorRuntimeError;

/// Compatibility alias for older C#-ported call sites.
pub type AkkaResult<T> = ActorRuntimeResult<T>;

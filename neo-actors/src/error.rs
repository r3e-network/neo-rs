use thiserror::Error;

/// Result type for actor operations.
pub type ActorRuntimeResult<T> = Result<T, ActorRuntimeError>;

/// Errors returned by the actor runtime implementation.
#[derive(Debug, Error)]
pub enum ActorRuntimeError {
    /// An actor's handler or lifecycle callback failed.
    #[error("actor failure: {0}")]
    Actor(String),
    /// A message could not be delivered to its target.
    #[error("message send failed: {0}")]
    Send(String),
    /// An `ask` request did not complete before its deadline.
    #[error("ask timed out")]
    AskTimeout,
    /// The runtime itself failed (scheduler, spawn, or shutdown).
    #[error("system failure: {0}")]
    System(String),
}

impl ActorRuntimeError {
    /// Builds an actor-failure error from any displayable cause.
    pub fn actor<E: ToString>(err: E) -> Self {
        Self::Actor(err.to_string())
    }

    /// Builds a send-failure error from any displayable cause.
    pub fn send<E: ToString>(err: E) -> Self {
        Self::Send(err.to_string())
    }

    /// Builds a system-failure error from any displayable cause.
    pub fn system<E: ToString>(err: E) -> Self {
        Self::System(err.to_string())
    }
}

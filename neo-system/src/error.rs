//! Error type used by the [`crate::Node`] builder and lifecycle.

use thiserror::Error;

/// Errors produced by the [`crate::NodeBuilder`] and the
/// [`crate::Node::run`] lifecycle.
#[derive(Debug, Error)]
pub enum NodeError {
    /// A required service was not set on the builder.
    #[error("missing required service: {0}")]
    MissingService(String),

    /// A required configuration was not provided.
    #[error("missing required configuration: {0}")]
    MissingConfig(String),

    /// Storage failed to initialise.
    #[error("storage initialisation failed: {0}")]
    Storage(String),

    /// A service failed to start.
    #[error("service start failed: {0}")]
    ServiceStart(String),

    /// Generic internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl NodeError {
    /// Construct from any string-like value.
    pub fn missing_service<E: ToString>(err: E) -> Self {
        NodeError::MissingService(err.to_string())
    }

    /// Construct from any string-like value.
    pub fn missing_config<E: ToString>(err: E) -> Self {
        NodeError::MissingConfig(err.to_string())
    }

    /// Construct from any string-like value.
    pub fn storage<E: ToString>(err: E) -> Self {
        NodeError::Storage(err.to_string())
    }

    /// Construct from any string-like value.
    pub fn service_start<E: ToString>(err: E) -> Self {
        NodeError::ServiceStart(err.to_string())
    }

    /// Construct from any string-like value.
    pub fn internal<E: ToString>(err: E) -> Self {
        NodeError::Internal(err.to_string())
    }
}

/// Result alias used by [`crate::NodeBuilder::build`] and
/// [`crate::Node::run`].
pub type NodeResult<T> = Result<T, NodeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_capture_message() {
        let err = NodeError::missing_service("consensus");
        assert_eq!(err.to_string(), "missing required service: consensus");
    }

    #[test]
    fn result_alias_compiles() {
        let ok: NodeResult<u32> = Ok(1);
        assert_eq!(ok.unwrap(), 1);
    }
}

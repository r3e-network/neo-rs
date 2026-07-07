//! Error types for deterministic script construction.

/// Errors raised while constructing a VM script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptBuilderError {
    /// The requested script-building operation is invalid.
    #[error("{0}")]
    InvalidOperation(String),
}

impl ScriptBuilderError {
    /// Creates an [`ScriptBuilderError::InvalidOperation`] from any message.
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::InvalidOperation(message.into())
    }
}

neo_error::impl_error_from_struct!(neo_error::CoreError, ScriptBuilderError => InvalidOperation);

/// Convenience result alias for fallible script-building operations.
pub type ScriptBuilderResult<T> = Result<T, ScriptBuilderError>;

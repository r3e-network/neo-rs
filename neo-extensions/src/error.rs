//! Error types for Neo Extensions
//!
//! Extension errors are now consolidated into `CoreError`.

// Re-export CoreError as the canonical extension error type.
pub use neo_error::CoreError;

/// Result type alias for extension operations, using `CoreError`.
pub type ExtensionResult<T> = Result<T, CoreError>;

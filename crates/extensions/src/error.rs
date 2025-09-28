//! Error types for Neo Extensions

use thiserror::Error;

/// Result type alias for extension operations.
pub type ExtensionResult<T> = Result<T, ExtensionError>;

/// Extension framework errors mirroring the C# plugin system exceptions.
#[derive(Error, Debug)]
pub enum ExtensionError {
    /// Attempted to register a plugin that already exists.
    #[error("Plugin '{0}' already exists")]
    PluginAlreadyExists(String),

    /// A declared dependency was not present when registering a plugin.
    #[error("Plugin '{plugin}' requires dependency '{dependency}'")]
    MissingDependency { plugin: String, dependency: String },

    /// Requested plugin was not found.
    #[error("Plugin '{0}' not found")]
    PluginNotFound(String),

    /// Plugin manager operations were invoked before initialization completed.
    #[error("Plugin system not initialized")]
    NotInitialized,

    /// Plugin initialization failed.
    #[error("Plugin initialization failed: {0}")]
    InitializationFailed(String),

    /// Generic plugin operation failure.
    #[error("Plugin operation failed: {0}")]
    OperationFailed(String),

    /// Configuration was invalid or malformed.
    #[error("Invalid plugin configuration: {0}")]
    InvalidConfiguration(String),

    /// Encoding/decoding error.
    #[error("Encoding error: {0}")]
    EncodingError(String),

    /// IO error propagated from the standard library.
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON serialisation/deserialisation error.
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Hexadecimal decoding error.
    #[error("Hex decoding error: {0}")]
    HexError(#[from] hex::FromHexError),

    /// Base64 decoding error.
    #[error("Base64 decoding error: {0}")]
    Base64Error(#[from] base64::DecodeError),

    /// Generic catch-all error.
    #[error("Extension error: {0}")]
    Generic(String),
}

impl ExtensionError {
    /// Helper for creating a generic error.
    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic(message.into())
    }

    /// Helper for creating an encoding error with a message.
    pub fn encoding(message: impl Into<String>) -> Self {
        Self::EncodingError(message.into())
    }

    /// Helper for creating an operation failure error with a message.
    pub fn operation_failed(message: impl Into<String>) -> Self {
        Self::OperationFailed(message.into())
    }

    /// Helper for creating an invalid configuration error with a message.
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration(message.into())
    }
}

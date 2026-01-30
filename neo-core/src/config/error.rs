//! Configuration error types

use std::path::PathBuf;
use thiserror::Error;

/// Configuration-related errors
#[derive(Debug, Error)]
pub enum ConfigError {
    /// File not found
    #[error("Configuration file not found: {0}")]
    FileNotFound(PathBuf),

    /// Failed to read file
    #[error("Failed to read configuration file: {0}")]
    ReadError(#[from] std::io::Error),

    /// TOML parsing error
    #[error("Failed to parse TOML: {0}")]
    TomlError(#[from] toml::de::Error),

    /// TOML serialization error
    #[error("Failed to serialize TOML: {0}")]
    TomlSerError(#[from] toml::ser::Error),

    /// Invalid configuration value
    #[error("Invalid configuration: {0}")]
    InvalidValue(String),

    /// Missing required field
    #[error("Missing required configuration field: {0}")]
    MissingField(String),

    /// Unknown network type
    #[error("Unknown network type: {0}")]
    UnknownNetwork(String),

    /// Genesis configuration error
    #[error("Genesis configuration error: {0}")]
    GenesisError(String),

    /// Protocol settings error
    #[error("Protocol settings error: {0}")]
    ProtocolError(String),

    /// Validation error
    #[error("Configuration validation failed: {0}")]
    ValidationError(String),
}

/// Result type for configuration operations
pub type ConfigResult<T> = Result<T, ConfigError>;

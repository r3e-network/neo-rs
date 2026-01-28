//! Telemetry error types

use thiserror::Error;

/// Telemetry-related errors
#[derive(Debug, Error)]
pub enum TelemetryError {
    /// Failed to initialize logging
    #[error("Failed to initialize logging: {0}")]
    LoggingInit(String),

    /// Failed to start metrics server
    #[error("Failed to start metrics server: {0}")]
    MetricsServer(String),

    /// Failed to collect system info
    #[error("Failed to collect system info: {0}")]
    SystemInfo(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Other error
    #[error("{0}")]
    Other(String),
}

impl From<anyhow::Error> for TelemetryError {
    fn from(e: anyhow::Error) -> Self {
        TelemetryError::Other(e.to_string())
    }
}

/// Result type for telemetry operations
pub type TelemetryResult<T> = Result<T, TelemetryError>;

//! Logging initialization and configuration

use crate::{TelemetryError, TelemetryResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter,
};

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,

    /// Log format (json, text, compact)
    pub format: LogFormat,

    /// Log file path (None for stdout only)
    pub file: Option<PathBuf>,

    /// Enable ANSI colors
    pub color: bool,

    /// Include target in log output
    pub include_target: bool,

    /// Include file location in log output
    pub include_location: bool,

    /// Include span events
    pub span_events: bool,
}

/// Log output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text format
    #[default]
    Text,
    /// Compact single-line format
    Compact,
    /// JSON format for machine parsing
    Json,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Text,
            file: None,
            color: true,
            include_target: true,
            include_location: false,
            span_events: false,
        }
    }
}

/// Initialize the logging system
pub fn init_logging(config: &LogConfig) -> TelemetryResult<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    let span_events = if config.span_events {
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    match config.format {
        LogFormat::Text => {
            let layer = fmt::layer()
                .with_ansi(config.color)
                .with_target(config.include_target)
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_span_events(span_events);

            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .try_init()
                .map_err(|e| TelemetryError::LoggingInit(e.to_string()))?;
        }
        LogFormat::Compact => {
            let layer = fmt::layer()
                .compact()
                .with_ansi(config.color)
                .with_target(config.include_target)
                .with_span_events(span_events);

            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .try_init()
                .map_err(|e| TelemetryError::LoggingInit(e.to_string()))?;
        }
        LogFormat::Json => {
            let layer = fmt::layer()
                .json()
                .with_target(config.include_target)
                .with_file(config.include_location)
                .with_line_number(config.include_location)
                .with_span_events(span_events);

            tracing_subscriber::registry()
                .with(filter)
                .with(layer)
                .try_init()
                .map_err(|e| TelemetryError::LoggingInit(e.to_string()))?;
        }
    }

    tracing::info!("Logging initialized with level: {}", config.level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LogConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Text);
        assert!(config.color);
    }
}

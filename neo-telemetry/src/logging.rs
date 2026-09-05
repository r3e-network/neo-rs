//! Logging initialization and configuration
//!
//! This module provides basic logging initialization.
//! For node-specific logging with file support, use `node_logging` module.

use crate::config::{LogFormat, LoggingConfig};
use crate::{TelemetryError, TelemetryResult};
use tracing_subscriber::{
    EnvFilter,
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// Initialize the logging system
///
/// This is the basic logging initialization. For node daemon logging
/// with file support and daemon mode, use `init_node_logging` from
/// the `node_logging` module.
pub fn init_logging(config: &LoggingConfig) -> TelemetryResult<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.level));

    let span_events = if config.include_location {
        // Reuse include_location flag for span events in basic logging
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    match config.format {
        LogFormat::Text | LogFormat::Pretty => {
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
    use crate::config::LoggingConfig;

    #[test]
    fn test_default_config() {
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Text);
        assert!(config.color);
    }
}

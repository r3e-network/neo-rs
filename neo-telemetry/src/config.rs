//! Configuration types for telemetry
//!
//! This module defines configuration types for the telemetry subsystem.
//! These types are independent of the main neo-config crate to avoid
//! circular dependencies.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Telemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable metrics collection
    #[serde(default)]
    pub metrics_enabled: bool,

    /// Metrics endpoint address
    #[serde(default = "default_metrics_address")]
    pub metrics_address: String,

    /// Metrics port
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,

    /// Enable health check endpoint
    #[serde(default)]
    pub health_enabled: bool,

    /// Health check endpoint port
    #[serde(default = "default_health_port")]
    pub health_port: u16,

    /// Enable Prometheus export
    #[serde(default)]
    pub prometheus_enabled: bool,

    /// Prometheus endpoint path
    #[serde(default = "default_prometheus_path")]
    pub prometheus_path: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            metrics_enabled: false,
            metrics_address: default_metrics_address(),
            metrics_port: default_metrics_port(),
            health_enabled: false,
            health_port: default_health_port(),
            prometheus_enabled: false,
            prometheus_path: default_prometheus_path(),
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (json, text, pretty)
    #[serde(default = "default_log_format")]
    pub format: LogFormat,

    /// Log file path (None for stdout only)
    pub file: Option<PathBuf>,

    /// Enable console output (disable for daemon mode)
    #[serde(default = "default_console_enabled")]
    pub console: bool,

    /// Enable ANSI colors
    #[serde(default = "default_color_enabled")]
    pub color: bool,

    /// Include target in log output
    #[serde(default = "default_true")]
    pub include_target: bool,

    /// Include file location in log output
    #[serde(default)]
    pub include_location: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: LogFormat::default(),
            file: None,
            console: default_console_enabled(),
            color: default_color_enabled(),
            include_target: true,
            include_location: false,
        }
    }
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
    /// Pretty printed format (multi-line)
    Pretty,
}

/// Complete telemetry and logging configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryAndLoggingConfig {
    /// Telemetry configuration
    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

// Default value functions
fn default_metrics_address() -> String {
    "127.0.0.1".to_string()
}

fn default_metrics_port() -> u16 {
    9090
}

fn default_health_port() -> u16 {
    8080
}

fn default_prometheus_path() -> String {
    "/metrics".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> LogFormat {
    LogFormat::Text
}

fn default_console_enabled() -> bool {
    true
}

fn default_color_enabled() -> bool {
    true
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert!(!config.metrics_enabled);
        assert_eq!(config.metrics_port, 9090);
    }

    #[test]
    fn test_logging_default() {
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert!(config.console);
    }

    #[test]
    fn test_log_format_serde() {
        let format = LogFormat::Json;
        let json = serde_json::to_string(&format).unwrap();
        assert_eq!(json, "\"json\"");

        let parsed: LogFormat = serde_json::from_str("\"json\"").unwrap();
        assert_eq!(parsed, LogFormat::Json);
    }
}

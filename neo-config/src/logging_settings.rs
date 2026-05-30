//! Logging configuration.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (json, text)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Log file path (None for stdout only)
    pub file: Option<PathBuf>,

    /// Enable color output
    #[serde(default = "default_color_enabled")]
    pub color: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

const fn default_color_enabled() -> bool {
    true
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file: None,
            color: default_color_enabled(),
        }
    }
}

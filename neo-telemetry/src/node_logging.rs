//! Node-specific logging initialization
//!
//! This module provides logging initialization for neo-node with support for:
//! - File output with rotation
//! - Daemon mode (no console output)
//! - Multiple output formats (JSON, text, pretty)
//! - Log level filtering

use crate::{LogFormat, LoggingConfig, TelemetryResult};
use anyhow::{Context, Result as AnyhowResult};
use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
use tracing_subscriber::{fmt, EnvFilter};

/// Guard for logging resources
///
/// This guard must be kept alive for the duration of the application
/// to ensure logs are flushed properly.
pub struct LoggingGuard {
    _guard: Option<WorkerGuard>,
}

impl LoggingGuard {
    /// Create a new guard (internal use)
    fn new(guard: Option<WorkerGuard>) -> Self {
        Self { _guard: guard }
    }

    /// Create an empty guard (for testing)
    pub fn empty() -> Self {
        Self { _guard: None }
    }
}

/// Initialize logging for the node daemon
///
/// This function sets up logging with file output support and daemon mode.
/// It's the primary logging initialization for neo-node.
///
/// # Arguments
///
/// * `config` - Logging configuration
/// * `daemon_mode` - If true, suppress console output
///
/// # Returns
///
/// Returns a `LoggingGuard` that must be kept alive for the duration of the application.
pub fn init_node_logging(
    config: &LoggingConfig,
    daemon_mode: bool,
) -> TelemetryResult<LoggingGuard> {
    use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};

    let level = &config.level;
    let filter_spec = format!("{level},neo={level}");
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter_spec));

    let mut guard = None;

    // Setup file writer if path is configured
    let file_writer = if let Some(ref path) = config.file {
        let path = path.to_string_lossy();
        let (writer, file_guard) = create_file_writer(&path)?;
        guard = Some(file_guard);
        Some(writer)
    } else {
        None
    };

    let has_file = file_writer.is_some();
    let console_enabled = config.console && !daemon_mode;

    // Combine writers based on configuration
    let writer: BoxMakeWriter = match (file_writer, console_enabled) {
        (Some(file), true) => BoxMakeWriter::new(io::stderr.and(file)),
        (Some(file), false) => BoxMakeWriter::new(file),
        (None, true) => BoxMakeWriter::new(io::stderr),
        (None, false) => BoxMakeWriter::new(io::sink),
    };

    let builder = fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(console_enabled && has_file);

    // Apply format
    match config.format {
        LogFormat::Json => {
            let _ = builder.json().try_init();
        }
        LogFormat::Pretty => {
            let _ = builder.pretty().try_init();
        }
        LogFormat::Compact => {
            let _ = builder.compact().try_init();
        }
        LogFormat::Text => {
            let _ = builder.try_init();
        }
    }

    Ok(LoggingGuard::new(guard))
}

/// Create a file writer for logging
fn create_file_writer(path: &str) -> AnyhowResult<(non_blocking::NonBlocking, WorkerGuard)> {
    let provided = Path::new(path);
    let file_path = if provided.is_file() || provided.extension().is_some() {
        provided.to_path_buf()
    } else {
        fs::create_dir_all(provided)
            .with_context(|| format!("failed to create log directory {}", provided.display()))?;
        provided.join(default_log_name())
    };

    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log directory {}", parent.display()))?;
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .with_context(|| format!("failed to open log file {}", file_path.display()))?;

    Ok(non_blocking(file))
}

/// Generate default log file name
fn default_log_name() -> String {
    format!("neo-node-{}.log", Local::now().format("%Y-%m-%d"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_log_name() {
        let name = default_log_name();
        assert!(name.starts_with("neo-node-"));
        assert!(name.ends_with(".log"));
    }

    #[test]
    fn test_logging_guard_empty() {
        let guard = LoggingGuard::empty();
        // Just verify it creates without panic
        drop(guard);
    }

    #[test]
    fn test_create_file_writer() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.log");

        let result = create_file_writer(&path.to_string_lossy());
        assert!(result.is_ok());

        // Verify file was created
        assert!(path.exists());
    }
}

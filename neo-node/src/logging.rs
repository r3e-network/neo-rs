//! Logging initialization for neo-node.

use crate::config::LoggingSection;
use anyhow::{Context, Result};
use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
use tracing_subscriber::{fmt, EnvFilter};

/// Handles for logging resources that need to be kept alive.
pub struct LoggingHandles {
    pub guard: Option<WorkerGuard>,
}

/// Initializes the tracing/logging subsystem.
pub fn init_tracing(logging: &LoggingSection, daemon_mode: bool) -> Result<LoggingHandles> {
    use tracing_subscriber::fmt::writer::{BoxMakeWriter, MakeWriterExt};

    if !logging.active {
        return Ok(LoggingHandles { guard: None });
    }

    let level = logging.level.as_deref().unwrap_or("info");
    let filter_spec = format!("{level},neo={level}");
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter_spec));

    let mut guard = None;

    let path_value = logging
        .file_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let file_requested = logging.file_enabled;
    let file_writer = if file_requested {
        let path = path_value.unwrap_or("Logs");
        let (writer, file_guard) = create_file_writer(path)?;
        guard = Some(file_guard);
        Some(writer)
    } else {
        None
    };

    let has_file = file_writer.is_some();
    let console_enabled = logging.console_output && !daemon_mode;

    let writer: BoxMakeWriter = match (file_writer, console_enabled) {
        (Some(file), true) => BoxMakeWriter::new(io::stderr.and(file)),
        (Some(file), false) => BoxMakeWriter::new(file),
        (None, true) => BoxMakeWriter::new(io::stderr),
        (None, false) => BoxMakeWriter::new(io::sink),
    };

    let builder = fmt()
        .with_env_filter(env_filter)
        .with_writer(writer)
        .with_ansi(console_enabled && !has_file);

    let normalized = logging
        .format
        .as_deref()
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "text".to_string());

    match normalized.as_str() {
        "json" => {
            let _ = builder.json().try_init();
        }
        "pretty" => {
            let _ = builder.pretty().try_init();
        }
        _ => {
            let _ = builder.try_init();
        }
    }
    Ok(LoggingHandles { guard })
}

fn create_file_writer(path: &str) -> Result<(non_blocking::NonBlocking, WorkerGuard)> {
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

fn default_log_name() -> String {
    format!("neo-node-{}.log", Local::now().format("%Y-%m-%d"))
}

//! # neo-node::node::logging
//!
//! Logging, tracing, and operator diagnostics setup.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use super::config::LoggingSection;

/// Guards that keep non-blocking file logging workers alive.
pub(super) struct LoggingGuards {
    _guards: Vec<WorkerGuard>,
}

#[derive(Clone, Copy)]
enum LogFormat {
    Pretty,
    Compact,
    Json,
}

pub(super) fn init_tracing(config: &LoggingSection) -> anyhow::Result<LoggingGuards> {
    let filter = logging_filter(config)?;
    let format = log_format(config.format.as_deref())?;
    let console_output = config.console_output.unwrap_or(true);
    let file_writer = config
        .file_path
        .as_deref()
        .map(|path| open_file_writer(path, config))
        .transpose()?;
    let file_writer_for_layer = file_writer.as_ref().map(|(writer, _guard)| writer.clone());

    match format {
        LogFormat::Pretty => {
            let console_layer = console_output.then(|| fmt::layer().pretty());
            let file_layer = file_writer_for_layer
                .map(|writer| fmt::layer().pretty().with_ansi(false).with_writer(writer));
            tracing_subscriber::registry()
                .with(filter)
                .with(console_layer)
                .with(file_layer)
                .try_init()
                .context("initializing tracing subscriber")?;
        }
        LogFormat::Compact => {
            let console_layer = console_output.then(|| fmt::layer().compact());
            let file_layer = file_writer_for_layer
                .map(|writer| fmt::layer().compact().with_ansi(false).with_writer(writer));
            tracing_subscriber::registry()
                .with(filter)
                .with(console_layer)
                .with(file_layer)
                .try_init()
                .context("initializing tracing subscriber")?;
        }
        LogFormat::Json => {
            let console_layer = console_output.then(|| fmt::layer().json());
            let file_layer = file_writer_for_layer
                .map(|writer| fmt::layer().json().with_ansi(false).with_writer(writer));
            tracing_subscriber::registry()
                .with(filter)
                .with(console_layer)
                .with(file_layer)
                .try_init()
                .context("initializing tracing subscriber")?;
        }
    }

    Ok(LoggingGuards {
        _guards: file_writer
            .into_iter()
            .map(|(_writer, guard)| guard)
            .collect(),
    })
}

fn logging_filter(config: &LoggingSection) -> anyhow::Result<EnvFilter> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let directive = logging_filter_directive(config, rust_log.as_deref());
    EnvFilter::try_new(&directive).with_context(|| format!("invalid logging filter {directive:?}"))
}

fn logging_filter_directive(config: &LoggingSection, rust_log: Option<&str>) -> String {
    if let Some(value) = rust_log.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    if !config.enabled {
        return "off".to_string();
    }
    let directive = config
        .level
        .as_deref()
        .map(str::trim)
        .filter(|level| !level.is_empty())
        .unwrap_or("info,neo=debug");
    directive.to_string()
}

fn log_format(format: Option<&str>) -> anyhow::Result<LogFormat> {
    match format
        .unwrap_or("pretty")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "pretty" => Ok(LogFormat::Pretty),
        "compact" => Ok(LogFormat::Compact),
        "json" => Ok(LogFormat::Json),
        other => {
            anyhow::bail!(
                "unsupported [logging].format {other:?}; expected pretty, compact, or json"
            );
        }
    }
}

fn open_file_writer(
    path: &Path,
    config: &LoggingSection,
) -> anyhow::Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(directory) = directory {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating log directory {}", directory.display()))?;
    }
    let file_name = path
        .file_name()
        .context("[logging].file_path must include a file name")?;
    if let Some(max_size) = config.max_file_size_bytes()? {
        let writer = SizeRotatingFileWriter::open(path, max_size, config.max_rotated_files())?;
        Ok(tracing_appender::non_blocking(writer))
    } else {
        let appender = tracing_appender::rolling::never(
            directory.unwrap_or_else(|| Path::new(".")),
            file_name,
        );
        Ok(tracing_appender::non_blocking(appender))
    }
}

struct SizeRotatingFileWriter {
    path: PathBuf,
    file: Option<File>,
    current_size: u64,
    max_size: u64,
    max_archives: usize,
}

impl SizeRotatingFileWriter {
    fn open(path: &Path, max_size: u64, max_archives: usize) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        if existing_len(path)? >= max_size {
            rotate_paths(path, max_archives)?;
        }
        let file = open_append(path)?;
        let current_size = existing_len(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Some(file),
            current_size,
            max_size,
            max_archives,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        rotate_paths(&self.path, self.max_archives)?;
        self.file = Some(open_append(&self.path)?);
        self.current_size = 0;
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "rotating log file is closed"))
    }
}

impl Write for SizeRotatingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current_size > 0
            && self
                .current_size
                .saturating_add(u64::try_from(buf.len()).unwrap_or(u64::MAX))
                > self.max_size
        {
            self.rotate()?;
        }
        let written = self.file_mut()?.write(buf)?;
        self.current_size = self.current_size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file_mut()?.flush()
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn existing_len(path: &Path) -> io::Result<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}

fn rotate_paths(path: &Path, max_archives: usize) -> io::Result<()> {
    if max_archives == 0 {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }

    let oldest = archive_path(path, max_archives);
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..max_archives).rev() {
        let from = archive_path(path, index);
        if from.exists() {
            std::fs::rename(from, archive_path(path, index + 1))?;
        }
    }
    if path.exists() {
        std::fs::rename(path, archive_path(path, 1))?;
    }
    Ok(())
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "neo-node.log".into());
    path.with_file_name(format!("{file_name}.{index}"))
}

#[cfg(test)]
#[path = "../../tests/node/logging.rs"]
mod tests;

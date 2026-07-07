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
//! - `filter`: tracing filter selection from `RUST_LOG` or node config.
//! - `format`: operator-facing log format parsing.
//! - `rotation`: file writer construction and size-based archive rotation.
//! - `tests`: Module-local tests and regression coverage.

mod filter;
mod format;
mod rotation;

use anyhow::Context;
#[cfg(test)]
use std::io::Write;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use super::config::LoggingSection;
use filter::logging_filter;
#[cfg(test)]
use filter::logging_filter_directive;
use format::{LogFormat, log_format};
use rotation::open_file_writer;
#[cfg(test)]
use rotation::{SizeRotatingFileWriter, archive_path};

/// Guards that keep non-blocking file logging workers alive.
pub(super) struct LoggingGuards {
    _guards: Vec<WorkerGuard>,
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

#[cfg(test)]
#[path = "../../tests/node/logging.rs"]
mod tests;

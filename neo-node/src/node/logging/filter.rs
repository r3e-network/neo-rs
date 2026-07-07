//! Tracing filter selection for node logging.
//!
//! `RUST_LOG` has precedence for operator overrides. The TOML logging section
//! provides the default directive when the environment is unset.

use anyhow::Context;
use tracing_subscriber::EnvFilter;

use crate::node::config::LoggingSection;

pub(super) fn logging_filter(config: &LoggingSection) -> anyhow::Result<EnvFilter> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let directive = logging_filter_directive(config, rust_log.as_deref());
    EnvFilter::try_new(&directive).with_context(|| format!("invalid logging filter {directive:?}"))
}

pub(super) fn logging_filter_directive(config: &LoggingSection, rust_log: Option<&str>) -> String {
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

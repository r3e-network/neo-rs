use std::path::{Path, PathBuf};

use anyhow::Context;
use neo_primitives::UnhandledExceptionPolicy;
use serde::Deserialize;

/// `[state_service]`: state-root/MPT support used by Neo's StateService plugin.
#[derive(Debug, Default, Deserialize)]
pub(in crate::node) struct StateServiceSection {
    /// Whether to start the state-service store and expose state RPC methods.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Whether to retain historical trie nodes for old-root proofs/state reads.
    #[serde(default, alias = "FullState")]
    pub(in crate::node) full_state: bool,
    /// Configured state-root store path.
    #[serde(default, alias = "Path")]
    pub(in crate::node) path: Option<PathBuf>,
}

/// `[indexer]`: read-side block/transaction/account indexing service.
#[derive(Debug, Deserialize)]
pub(in crate::node) struct IndexerSection {
    /// Whether to start the indexer service and expose indexer RPC methods.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Optional JSON snapshot path for the indexer.
    #[serde(default, alias = "Path")]
    pub(in crate::node) path: Option<PathBuf>,
    /// Optional RocksDB/service-store path for the indexer.
    #[serde(default, alias = "StorePath", alias = "DBPath", alias = "DbPath")]
    pub(in crate::node) store_path: Option<PathBuf>,
    /// Whether to rebuild the in-memory index from the current canonical chain
    /// on startup before tailing live block import events.
    #[serde(default = "super::default_true", alias = "BackfillOnStartup")]
    pub(in crate::node) backfill_on_startup: bool,
}

impl Default for IndexerSection {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            store_path: None,
            backfill_on_startup: true,
        }
    }
}

/// `[application_logs]`: C# ApplicationLogs plugin-compatible storage.
#[derive(Debug, Default, Deserialize)]
pub(in crate::node) struct ApplicationLogsSection {
    /// Whether to capture ApplicationExecuted logs for RPC queries.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Plugin database path. Accepts `{0}` as uppercase network magic.
    #[serde(default, alias = "Path")]
    path: Option<PathBuf>,
    /// Maximum serialized stack item size.
    #[serde(default, alias = "MaxStackSize")]
    max_stack_size: Option<usize>,
    /// Whether to include ApplicationEngine.Log messages.
    #[serde(default, alias = "Debug")]
    debug: bool,
    /// Plugin exception handling policy.
    #[serde(default, alias = "UnhandledExceptionPolicy")]
    exception_policy: Option<UnhandledExceptionPolicy>,
}

impl ApplicationLogsSection {
    pub(in crate::node) fn settings(
        &self,
        network: u32,
    ) -> neo_rpc::application_logs::ApplicationLogsSettings {
        let mut settings = neo_rpc::application_logs::ApplicationLogsSettings::default();
        let default_path = settings.path.clone();
        settings.enabled = self.enabled;
        settings.network = network;
        settings.path = configured_path_or_default(self.path.as_deref(), &default_path, network);
        if let Some(max_stack_size) = self.max_stack_size {
            settings.max_stack_size = max_stack_size;
        }
        settings.debug = self.debug;
        if let Some(policy) = self.exception_policy {
            settings.exception_policy = policy;
        }
        settings
    }
}

/// `[tokens_tracker]`: NEP-11/NEP-17 balance and transfer indexing.
#[derive(Debug, Default, Deserialize)]
pub(in crate::node) struct TokensTrackerSection {
    /// Whether to track NEP-11/NEP-17 balances and transfers.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Plugin database path. Accepts `{0}` as uppercase network magic.
    #[serde(default, alias = "DBPath", alias = "Path")]
    db_path: Option<PathBuf>,
    /// Whether to retain historical transfer records.
    #[serde(default, alias = "TrackHistory")]
    track_history: Option<bool>,
    /// Maximum RPC result count.
    #[serde(default, alias = "MaxResults")]
    max_results: Option<u32>,
    /// Optional network override; defaults to the node network.
    #[serde(default, alias = "Network")]
    network: Option<u32>,
    /// Enabled standards, usually `NEP-17` and `NEP-11`.
    #[serde(default, alias = "EnabledTrackers")]
    enabled_trackers: Vec<String>,
    /// Plugin exception handling policy.
    #[serde(default, alias = "UnhandledExceptionPolicy")]
    exception_policy: Option<UnhandledExceptionPolicy>,
}

impl TokensTrackerSection {
    pub(in crate::node) fn settings(
        &self,
        network: u32,
    ) -> neo_rpc::plugins::tokens_tracker::TokensTrackerSettings {
        let mut settings = neo_rpc::plugins::tokens_tracker::TokensTrackerSettings::default();
        let default_path = settings.db_path.clone();
        settings.network = self.network.unwrap_or(network);
        settings.db_path =
            configured_path_or_default(self.db_path.as_deref(), &default_path, settings.network);
        if let Some(track_history) = self.track_history {
            settings.track_history = track_history;
        }
        if let Some(max_results) = self.max_results {
            settings.max_results = max_results;
        }
        settings.enabled_trackers =
            neo_rpc::plugins::tokens_tracker::TokensTrackerSettings::normalize_enabled_trackers(
                &self.enabled_trackers,
            );
        if let Some(policy) = self.exception_policy {
            settings.exception_policy = policy;
        }
        settings
    }
}

/// `[telemetry]`: local metrics endpoints.
#[derive(Debug, Default, Deserialize)]
pub(in crate::node) struct TelemetrySection {
    /// Prometheus-compatible metrics endpoint.
    #[serde(default)]
    pub(in crate::node) metrics: TelemetryMetricsSection,
}

/// `[telemetry.metrics]`: Prometheus text exporter.
#[derive(Debug, Clone, Default, Deserialize)]
pub(in crate::node) struct TelemetryMetricsSection {
    /// Whether to start the metrics endpoint.
    #[serde(default, alias = "Enabled")]
    pub(in crate::node) enabled: bool,
    /// Metrics TCP port.
    #[serde(default, alias = "Port")]
    pub(in crate::node) port: Option<u16>,
    /// Metrics bind address.
    #[serde(default, alias = "BindAddress")]
    pub(in crate::node) bind_address: Option<String>,
    /// HTTP path that serves Prometheus text.
    #[serde(default, alias = "Path")]
    pub(in crate::node) path: Option<String>,
}

pub(in crate::node) const TELEMETRY_HEALTH_PATH: &str = "/healthz";
pub(in crate::node) const TELEMETRY_READY_PATH: &str = "/readyz";

impl TelemetryMetricsSection {
    pub(in crate::node) fn bind_socket_addr(&self) -> anyhow::Result<std::net::SocketAddr> {
        let bind_address = self
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1")
            .parse()
            .context("invalid [telemetry.metrics].bind_address")?;
        Ok(std::net::SocketAddr::new(
            bind_address,
            self.port.unwrap_or(9090),
        ))
    }

    pub(in crate::node) fn endpoint_path(&self) -> &str {
        self.path.as_deref().unwrap_or("/metrics")
    }
}

/// `[logging]`: tracing subscriber configuration.
#[derive(Debug, Clone, Deserialize)]
pub(in crate::node) struct LoggingSection {
    /// Whether TOML-driven logging is active. `RUST_LOG` still overrides
    /// the filter directive when present.
    #[serde(default = "super::default_true", alias = "Enabled", alias = "active")]
    pub(in crate::node) enabled: bool,
    /// Tracing filter directive, e.g. `info` or `info,neo=debug`.
    #[serde(default, alias = "Level")]
    pub(in crate::node) level: Option<String>,
    /// `pretty`, `compact`, or `json`.
    #[serde(default, alias = "Format")]
    pub(in crate::node) format: Option<String>,
    /// Optional file path. When present, logs are also written to this file.
    #[serde(default, alias = "FilePath")]
    pub(in crate::node) file_path: Option<PathBuf>,
    /// Whether to log to stdout/stderr. Defaults to true.
    #[serde(default, alias = "ConsoleOutput")]
    pub(in crate::node) console_output: Option<bool>,
    /// Rotate the log file once it reaches this size, e.g. `100MB`.
    #[serde(default, alias = "MaxFileSize")]
    pub(in crate::node) max_file_size: Option<String>,
    /// Number of rotated log archives to retain when `max_file_size` is set.
    #[serde(default, alias = "MaxFiles")]
    pub(in crate::node) max_files: Option<usize>,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            enabled: true,
            level: None,
            format: None,
            file_path: None,
            console_output: None,
            max_file_size: None,
            max_files: None,
        }
    }
}

impl LoggingSection {
    pub(in crate::node) fn max_file_size_bytes(&self) -> anyhow::Result<Option<u64>> {
        self.max_file_size
            .as_deref()
            .map(parse_log_size)
            .transpose()
    }

    pub(in crate::node) fn max_rotated_files(&self) -> usize {
        self.max_files.unwrap_or(5)
    }
}

fn parse_log_size(raw: &str) -> anyhow::Result<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("[logging].max_file_size must not be empty");
    }

    let number_len = raw
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '_')
        .map(char::len_utf8)
        .sum::<usize>();
    let number = raw[..number_len].replace('_', "");
    let suffix = raw[number_len..].trim().to_ascii_lowercase();
    if number.is_empty() {
        anyhow::bail!("[logging].max_file_size must start with an integer byte count");
    }
    let value = number
        .parse::<u64>()
        .with_context(|| format!("invalid [logging].max_file_size value {raw:?}"))?;
    let multiplier = match suffix.as_str() {
        "" | "b" | "byte" | "bytes" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        other => {
            anyhow::bail!(
                "unsupported [logging].max_file_size suffix {other:?}; expected B, KB, MB, or GB"
            );
        }
    };
    value
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("[logging].max_file_size value {raw:?} overflows u64 bytes"))
}

fn configured_path_or_default(path: Option<&Path>, default_path: &str, network: u32) -> String {
    super::network_scoped_path(path.unwrap_or_else(|| Path::new(default_path)), network)
        .to_string_lossy()
        .into_owned()
}

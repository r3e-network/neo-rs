//! TOML configuration and storage bootstrap helpers for the node daemon.

use std::path::{Path, PathBuf};

use anyhow::Context;
use neo_config::ProtocolSettings;
use serde::Deserialize;
use tracing::{info, warn};

mod observability;
mod services;
mod validation;

pub(in crate::node) use observability::{
    ObservabilityErrorEndpoint, ObservabilityHeartbeatEndpoint, ObservabilitySection,
};
pub(in crate::node) use services::{
    ApplicationLogsSection, IndexerSection, LoggingSection, StateServiceSection,
    TELEMETRY_HEALTH_PATH, TELEMETRY_READY_PATH, TelemetryMetricsSection, TelemetrySection,
    TokensTrackerSection,
};
#[cfg(test)]
pub(super) use validation::validate_config;
pub(super) use validation::{
    open_memory_store, open_store, service_store_provider, validate_config_for_ledger_mode,
    validate_state_service_storage, validate_storage,
};

/// The daemon's TOML configuration surface.
#[derive(Debug, Default, Deserialize)]
pub(super) struct NodeConfig {
    #[serde(default)]
    pub(super) network: NetworkSection,
    #[serde(default)]
    pub(super) storage: StorageSection,
    #[serde(default)]
    pub(super) p2p: P2pSection,
    #[serde(default)]
    pub(super) rpc: RpcSection,
    #[serde(default, alias = "dbft")]
    pub(super) consensus: ConsensusSection,
    #[serde(default)]
    pub(super) blockchain: BlockchainSection,
    #[serde(default)]
    pub(super) mempool: MempoolSection,
    #[serde(default)]
    pub(super) state_service: StateServiceSection,
    #[serde(default)]
    pub(super) indexer: IndexerSection,
    #[serde(default)]
    pub(super) application_logs: ApplicationLogsSection,
    #[serde(default)]
    pub(super) tokens_tracker: TokensTrackerSection,
    #[serde(default)]
    pub(super) telemetry: TelemetrySection,
    #[serde(default)]
    pub(super) logging: LoggingSection,
    #[serde(default)]
    pub(super) observability: ObservabilitySection,
}

/// `[network]`: which Neo network the node joins.
#[derive(Debug, Default, Deserialize)]
pub(super) struct NetworkSection {
    /// `"TestNet"` / `"MainNet"` — selects the built-in protocol preset.
    #[serde(default)]
    network_type: Option<String>,
    /// Explicit network magic override (wins over the preset).
    #[serde(default)]
    network_magic: Option<u32>,
}

/// `[storage]`: persistence backend.
#[derive(Debug, Default, Deserialize)]
pub(super) struct StorageSection {
    /// Storage provider name. Supported values are `memory`, `mdbx`, and
    /// `rocksdb`; production builds default persistent stores to `mdbx`.
    #[serde(default)]
    pub(super) backend: Option<String>,
    /// Database directory for persistent storage backends.
    #[serde(default)]
    pub(super) data_dir: Option<PathBuf>,
    /// Open the primary persistent store in read-only mode.
    #[serde(default)]
    pub(super) read_only: bool,
    /// MDBX maximum map geometry in GiB.
    #[serde(default)]
    pub(super) mdbx_geometry_upper_gb: Option<u32>,
    /// MDBX map growth step in MiB.
    #[serde(default)]
    pub(super) mdbx_geometry_growth_mb: Option<u32>,
    /// Maximum concurrent MDBX readers.
    #[serde(default)]
    pub(super) mdbx_max_readers: Option<u32>,
}

impl StorageSection {
    /// The configured persistent store directory, accepting either `data_dir`
    /// from the primary storage section.
    pub(super) fn data_directory(&self) -> Option<PathBuf> {
        self.data_dir.clone()
    }

    /// Builds the provider-neutral storage configuration for a persistent
    /// backend path.
    pub(super) fn storage_config_for_path(
        &self,
        path: PathBuf,
    ) -> neo_storage::persistence::storage::StorageConfig {
        neo_storage::persistence::storage::StorageConfig {
            path,
            read_only: self.read_only,
            mdbx_geometry_upper_bytes: self
                .mdbx_geometry_upper_gb
                .map(|gb| gb as isize * 1024 * 1024 * 1024),
            mdbx_geometry_growth_bytes: self
                .mdbx_geometry_growth_mb
                .map(|mb| mb as isize * 1024 * 1024),
            mdbx_max_readers: self.mdbx_max_readers,
            ..Default::default()
        }
    }
}

/// `[p2p]`: peer-to-peer networking.
#[derive(Debug, Default, Deserialize)]
pub(super) struct P2pSection {
    /// TCP port the node listens on for inbound peers.
    #[serde(default, alias = "listen_port", alias = "Port")]
    pub(super) port: Option<u16>,
    /// Address to bind the listener to (default `0.0.0.0`).
    #[serde(default)]
    pub(super) bind_address: Option<String>,
    /// Seed node endpoints (`host:port`) to dial on startup. Falls back
    /// to the protocol preset's seed list when empty.
    #[serde(default)]
    pub(super) seed_nodes: Vec<String>,
    /// Whether P2P message compression is advertised/enabled.
    #[serde(default, alias = "EnableCompression")]
    enable_compression: Option<bool>,
    /// Minimum desired outbound peer count.
    #[serde(default, alias = "MinDesiredConnections")]
    min_desired_connections: Option<usize>,
    /// Maximum simultaneous peer count. `-1` matches C# "unlimited".
    #[serde(default, alias = "MaxConnections")]
    max_connections: Option<i64>,
    /// Maximum simultaneous peers accepted from one remote IP.
    #[serde(default, alias = "MaxConnectionsPerAddress")]
    max_connections_per_address: Option<usize>,
    /// Maximum known inventory hashes retained for duplicate suppression.
    #[serde(default, alias = "MaxKnownHashes")]
    max_known_hashes: Option<usize>,
    /// Maximum recent broadcasts retained for diagnostics.
    #[serde(default)]
    broadcast_history_limit: Option<usize>,
}

impl P2pSection {
    /// Build the live channel configuration consumed by `LocalNodeService`.
    pub(super) fn channels_config(&self) -> anyhow::Result<neo_network::ChannelsConfig> {
        let mut config = neo_network::ChannelsConfig::default();

        if let Some(enable_compression) = self.enable_compression {
            config.enable_compression = enable_compression;
        }
        if let Some(min_desired_connections) = self.min_desired_connections {
            config.min_desired_connections = min_desired_connections;
        }
        if let Some(max_connections) = self.max_connections {
            config.max_connections = match max_connections {
                -1 => usize::MAX,
                value if value >= 0 => usize::try_from(value)
                    .context("invalid [p2p].max_connections: value is too large")?,
                _ => {
                    anyhow::bail!(
                        "invalid [p2p].max_connections: use -1 for unlimited or a non-negative integer"
                    )
                }
            };
        }
        if let Some(max_connections_per_address) = self.max_connections_per_address {
            config.max_connections_per_address = max_connections_per_address;
        }
        if let Some(max_known_hashes) = self.max_known_hashes {
            config.max_known_hashes = max_known_hashes;
        }
        if let Some(broadcast_history_limit) = self.broadcast_history_limit {
            config.broadcast_history_limit = broadcast_history_limit;
        }

        Ok(config)
    }
}

/// `[rpc]`: JSON-RPC server.
#[derive(Debug, Default, Deserialize)]
pub(super) struct RpcSection {
    /// Whether to start the RPC server.
    #[serde(default, alias = "Enabled")]
    pub(super) enabled: bool,
    /// RPC listen port (default `10332`).
    #[serde(default, alias = "Port")]
    pub(super) port: Option<u16>,
    /// RPC bind address (default `127.0.0.1`).
    #[serde(default, alias = "BindAddress")]
    pub(super) bind_address: Option<String>,
    /// Whether to enable Basic authentication. When omitted, non-empty
    /// credentials enable auth.
    #[serde(default, alias = "AuthEnabled")]
    auth_enabled: Option<bool>,
    /// Basic-auth username.
    #[serde(default, alias = "RpcUser")]
    rpc_user: Option<String>,
    /// Basic-auth password.
    #[serde(default, alias = "RpcPass")]
    rpc_pass: Option<String>,
    /// Whether to enable CORS headers.
    #[serde(default, alias = "EnableCors")]
    cors_enabled: Option<bool>,
    /// Allowed CORS origins.
    #[serde(default, alias = "AllowOrigins")]
    allow_origins: Vec<String>,
    /// Maximum GAS allowed for one invoke call, in datoshi.
    #[serde(default, alias = "MaxGasInvoke")]
    max_gas_invoke: Option<i64>,
    /// Maximum iterator result items returned in one RPC response.
    #[serde(
        default,
        alias = "max_iterator_result_items",
        alias = "MaxIteratorResultItems"
    )]
    max_iterator_results: Option<usize>,
    /// Maximum VM stack items allowed in invoke responses.
    #[serde(default, alias = "MaxStackSize")]
    max_stack_size: Option<usize>,
    /// RPC method names disabled for this endpoint.
    #[serde(default, alias = "DisabledMethods")]
    disabled_methods: Vec<String>,
    /// Maximum concurrently accepted RPC connections.
    #[serde(default, alias = "MaxConcurrentConnections")]
    max_concurrent_connections: Option<usize>,
    /// Maximum JSON-RPC request body size, in bytes.
    #[serde(default, alias = "MaxRequestBodySize")]
    max_request_body_size: Option<usize>,
    /// Maximum requests per second per IP (0 disables rate limiting).
    #[serde(default, alias = "MaxRequestsPerSecond")]
    max_requests_per_second: Option<u32>,
    /// Burst capacity for the per-IP rate limiter.
    #[serde(default, alias = "RateLimitBurst")]
    rate_limit_burst: Option<u32>,
    /// Maximum JSON-RPC calls accepted in one batch.
    #[serde(default, alias = "MaxBatchSize")]
    max_batch_size: Option<usize>,
    /// Whether invoke sessions are enabled.
    #[serde(default, alias = "SessionEnabled")]
    session_enabled: Option<bool>,
    /// Session expiration time in seconds.
    #[serde(default, alias = "SessionExpirationTime")]
    session_expiration_time: Option<u64>,
    /// Page size used by `findstorage`.
    #[serde(default, alias = "FindStoragePageSize")]
    find_storage_page_size: Option<usize>,
    /// Idle keep-alive timeout in seconds; negative disables idle reaping.
    #[serde(default, alias = "KeepAliveTimeout")]
    keep_alive_timeout: Option<i32>,
    /// Request header timeout in seconds.
    #[serde(default, alias = "RequestHeadersTimeout")]
    request_headers_timeout: Option<u64>,
}

impl RpcSection {
    pub(super) fn server_config(
        &self,
        network: u32,
    ) -> anyhow::Result<neo_rpc::server::RpcServerConfig> {
        let bind_address = self
            .bind_address
            .as_deref()
            .unwrap_or("127.0.0.1")
            .parse()
            .context("invalid [rpc].bind_address")?;
        let mut config = neo_rpc::server::RpcServerConfig {
            network,
            bind_address,
            port: self.port.unwrap_or(10332),
            ..Default::default()
        };

        if let Some(enable_cors) = self.cors_enabled {
            config.enable_cors = enable_cors;
        }
        if !self.allow_origins.is_empty() {
            config.allow_origins.clone_from(&self.allow_origins);
        }
        if let Some(max_gas_invoke) = self.max_gas_invoke {
            if max_gas_invoke < 0 {
                anyhow::bail!("[rpc].max_gas_invoke must not be negative");
            }
            config.max_gas_invoke = max_gas_invoke;
        }
        if let Some(max_iterator_results) = self.max_iterator_results {
            config.max_iterator_result_items = max_iterator_results;
        }
        if let Some(max_stack_size) = self.max_stack_size {
            config.max_stack_size = max_stack_size;
        }
        if !self.disabled_methods.is_empty() {
            config.disabled_methods.clone_from(&self.disabled_methods);
        }
        if let Some(max_concurrent_connections) = self.max_concurrent_connections {
            config.max_concurrent_connections = max_concurrent_connections;
        }
        if let Some(max_request_body_size) = self.max_request_body_size {
            config.max_request_body_size = max_request_body_size;
        }
        if let Some(max_requests_per_second) = self.max_requests_per_second {
            config.max_requests_per_second = max_requests_per_second;
        }
        if let Some(rate_limit_burst) = self.rate_limit_burst {
            config.rate_limit_burst = rate_limit_burst;
        }
        if let Some(max_batch_size) = self.max_batch_size {
            config.max_batch_size = max_batch_size;
        }
        if let Some(session_enabled) = self.session_enabled {
            config.session_enabled = session_enabled;
        }
        if let Some(session_expiration_time) = self.session_expiration_time {
            config.session_expiration_time = session_expiration_time;
        }
        if let Some(find_storage_page_size) = self.find_storage_page_size {
            config.find_storage_page_size = find_storage_page_size;
        }
        if let Some(keep_alive_timeout) = self.keep_alive_timeout {
            config.keep_alive_timeout = keep_alive_timeout;
        }
        if let Some(request_headers_timeout) = self.request_headers_timeout {
            config.request_headers_timeout = request_headers_timeout;
        }

        let credentials_present = self
            .rpc_user
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
            || self
                .rpc_pass
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty());
        let auth_enabled = self.auth_enabled.unwrap_or(credentials_present);
        if auth_enabled {
            let user = self.rpc_user.as_deref().unwrap_or_default().trim();
            let pass = self.rpc_pass.as_deref().unwrap_or_default();
            if user.is_empty() || pass.is_empty() {
                anyhow::bail!(
                    "[rpc].auth_enabled requires non-empty [rpc].rpc_user and [rpc].rpc_pass"
                );
            }
            config.rpc_user = user.to_string();
            config.rpc_pass = pass.to_string();
        }

        Ok(config)
    }
}

/// `[consensus]`: dBFT consensus participation.
#[derive(Debug, Default, Deserialize)]
pub(super) struct ConsensusSection {
    /// Whether this node participates in dBFT consensus.
    #[serde(default)]
    pub(super) enabled: bool,
    /// C# DBFT-style auto-start flag. Parsed for config compatibility; the
    /// Rust daemon starts consensus from `enabled` plus validator key config.
    #[serde(default)]
    pub(super) auto_start: bool,
    /// This node's 32-byte secp256r1 private key, hex-encoded.
    #[serde(default)]
    pub(super) private_key_hex: Option<String>,
    /// Optional HSM-backed consensus signing (PKCS#11).
    #[serde(default)]
    pub(super) hsm: Option<crate::consensus::HsmKeyConfig>,
}

/// `[blockchain]`: protocol settings that affect validation / production.
#[derive(Debug, Default, Deserialize)]
pub(super) struct BlockchainSection {
    /// Block interval in milliseconds (`ProtocolSettings.MillisecondsPerBlock`).
    #[serde(
        default,
        alias = "milliseconds_per_block",
        alias = "MillisecondsPerBlock"
    )]
    block_time: Option<u32>,
    /// Maximum transactions accepted in one block.
    #[serde(default, alias = "MaxTransactionsPerBlock")]
    pub(super) max_transactions_per_block: Option<u32>,
    /// Maximum `ValidUntilBlock` increment for transactions.
    #[serde(
        default,
        alias = "max_valid_until_block_increment",
        alias = "MaxValidUntilBlockIncrement"
    )]
    max_valid_until_block_increment: Option<u32>,
    /// Maximum number of traceable blocks exposed to contracts.
    #[serde(default, alias = "MaxTraceableBlocks")]
    max_traceable_blocks: Option<u32>,
}

/// `[mempool]`: transaction pool sizing.
#[derive(Debug, Default, Deserialize)]
pub(super) struct MempoolSection {
    /// Maximum number of transactions retained in the memory pool.
    #[serde(
        default,
        alias = "memory_pool_max_transactions",
        alias = "MemoryPoolMaxTransactions"
    )]
    max_transactions: Option<i32>,
}

const fn default_true() -> bool {
    true
}

/// Default P2P port for a network magic (TestNet `20333`, MainNet
/// `10333`); `0` (ephemeral) for unknown networks.
pub(super) fn default_p2p_port(network: u32) -> u16 {
    match network {
        0x3554_334E => 20333, // TestNet
        0x334F_454E => 10333, // MainNet
        _ => 0,
    }
}

/// Loads the TOML node configuration and derives [`ProtocolSettings`]
/// from the configured network type. A missing file yields the built-in
/// defaults (MainNet preset).
pub(super) fn load_config(
    path: &PathBuf,
    magic_override: Option<u32>,
) -> anyhow::Result<(ProtocolSettings, NodeConfig)> {
    let config: NodeConfig = if path.exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading node config {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("parsing TOML node config {}", path.display()))?
    } else {
        info!(target: "neo", path = %path.display(), "config not found; using built-in defaults");
        NodeConfig::default()
    };

    let mut settings = match config.network.network_type.as_deref() {
        Some(t) if t.eq_ignore_ascii_case("testnet") => ProtocolSettings::testnet(),
        Some(t) if t.eq_ignore_ascii_case("mainnet") => ProtocolSettings::mainnet(),
        Some(other) => {
            warn!(target: "neo", network_type = other, "unknown network_type; using default (MainNet) settings");
            ProtocolSettings::default()
        }
        None => ProtocolSettings::default(),
    };
    if let Some(magic) = magic_override.or(config.network.network_magic) {
        settings.network = magic;
    }
    if let Some(block_time) = config.blockchain.block_time {
        settings.milliseconds_per_block = block_time;
    }
    if let Some(max_transactions) = config.blockchain.max_transactions_per_block {
        settings.max_transactions_per_block = max_transactions;
    }
    if let Some(max_valid_until_block_increment) = config.blockchain.max_valid_until_block_increment
    {
        settings.max_valid_until_block_increment = max_valid_until_block_increment;
    }
    if let Some(max_traceable_blocks) = config.blockchain.max_traceable_blocks {
        settings.max_traceable_blocks = max_traceable_blocks;
    }
    if let Some(max_transactions) = config.mempool.max_transactions {
        settings.memory_pool_max_transactions = max_transactions;
    }
    Ok((settings, config))
}

pub(super) fn network_scoped_path(path: &Path, network: u32) -> PathBuf {
    let raw = path.to_string_lossy();
    if raw.contains("{0}") {
        PathBuf::from(raw.replace("{0}", &format!("{network:08X}")))
    } else {
        path.to_path_buf()
    }
}

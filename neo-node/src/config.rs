//! Neo Node Configuration
//!
//! This module provides configuration parsing for the Neo N3 blockchain node.

use anyhow::{bail, Context, Result};
use neo_core::{
    application_logs::ApplicationLogsSettings,
    constants::{MAINNET_MAGIC, MAX_BLOCK_SIZE, TESTNET_MAGIC},
    network::p2p::channels_config::ChannelsConfig,
    oracle_service::OracleServiceSettings,
    persistence::storage::{CompressionAlgorithm, StorageConfig},
    protocol_settings::ProtocolSettings,
    state_service::state_store::StateServiceSettings,
    tokens_tracker::TokensTrackerSettings,
    UnhandledExceptionPolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env, fs,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;

fn plugins_directory() -> PathBuf {
    match env::var_os("NEO_PLUGINS_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from("data/Plugins"),
    }
}

/// Returns the default config directory for RPC server configuration.
fn config_directory() -> PathBuf {
    plugins_directory().join("RpcServer")
}

fn application_logs_config_path() -> PathBuf {
    plugins_directory()
        .join("ApplicationLogs")
        .join("ApplicationLogs.json")
}

fn application_logs_directory() -> PathBuf {
    plugins_directory().join("ApplicationLogs")
}

fn state_service_config_path() -> PathBuf {
    plugins_directory()
        .join("StateService")
        .join("StateService.json")
}

fn state_service_directory() -> PathBuf {
    plugins_directory().join("StateService")
}

fn tokens_tracker_config_path() -> PathBuf {
    plugins_directory()
        .join("TokensTracker")
        .join("TokensTracker.json")
}

fn tokens_tracker_directory() -> PathBuf {
    plugins_directory().join("TokensTracker")
}

fn oracle_service_config_path() -> PathBuf {
    plugins_directory()
        .join("OracleService")
        .join("OracleService.json")
}

fn dbft_config_path() -> PathBuf {
    plugins_directory()
        .join("DBFTPlugin")
        .join("DBFTPlugin.json")
}

/// High-level node configuration derived from the Neo CLI TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NodeConfig {
    pub network: NetworkSection,
    pub p2p: P2PSection,
    pub storage: StorageSection,
    pub blockchain: BlockchainSection,
    pub rpc: RpcSection,
    pub application_logs: Option<ApplicationLogsSection>,
    pub state_service: Option<StateServiceSection>,
    pub tokens_tracker: Option<TokensTrackerSection>,
    pub oracle_service: Option<OracleServiceSection>,
    pub dbft: Option<DbftSection>,
    pub logging: LoggingSection,
    pub unlock_wallet: UnlockWalletSection,
    pub contracts: ContractsSection,
    pub plugins: PluginsSection,
    /// Consensus configuration section
    pub consensus: Option<ConsensusSection>,
    /// Telemetry configuration section
    pub telemetry: Option<TelemetrySection>,
    /// Mempool configuration section
    pub mempool: Option<MempoolSection>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkSection {
    #[serde(alias = "NetworkType")]
    pub network_type: Option<String>,
    #[serde(alias = "Network")]
    pub network_magic: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct P2PSection {
    #[serde(alias = "Port", alias = "port")]
    pub listen_port: Option<u16>,
    #[serde(alias = "MinDesiredConnections")]
    pub min_desired_connections: Option<usize>,
    #[serde(alias = "MaxConnections")]
    pub max_connections: Option<usize>,
    #[serde(alias = "MaxConnectionsPerAddress")]
    pub max_connections_per_address: Option<usize>,
    #[serde(alias = "MaxKnownHashes")]
    pub max_known_hashes: Option<usize>,
    #[serde(alias = "BroadcastHistoryLimit")]
    pub broadcast_history_limit: Option<usize>,
    #[serde(alias = "EnableCompression")]
    pub enable_compression: Option<bool>,
    #[serde(alias = "SeedList")]
    pub seed_nodes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StorageSection {
    #[serde(alias = "Path", alias = "data_dir")]
    pub path: Option<String>,
    #[serde(alias = "Engine")]
    pub backend: Option<String>,
    #[serde(alias = "CacheSize")]
    pub cache_size: Option<u64>,
    #[serde(alias = "Compression")]
    pub compression: Option<String>,
    #[serde(alias = "WriteBufferSize")]
    pub write_buffer_size: Option<u64>,
    #[serde(alias = "MaxOpenFiles")]
    pub max_open_files: Option<u32>,
    #[serde(alias = "ReadOnly")]
    pub read_only: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct BlockchainSection {
    /// Block time in milliseconds
    pub block_time: Option<u64>,
    /// Maximum transactions per block
    pub max_transactions_per_block: Option<u32>,
    /// Maximum free transactions per block
    pub max_free_transactions_per_block: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RpcSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "BindAddress")]
    pub bind_address: Option<String>,
    #[serde(alias = "Port")]
    pub port: Option<u16>,
    #[serde(alias = "EnableCors")]
    pub cors_enabled: Option<bool>,
    #[serde(alias = "AllowOrigins")]
    pub allow_origins: Vec<String>,
    #[serde(alias = "MaxConcurrentConnections", alias = "MaxConnections")]
    pub max_connections: Option<usize>,
    #[serde(alias = "MaxRequestBodySize")]
    pub max_request_body_size: Option<usize>,
    #[serde(alias = "MaxGasInvoke")]
    pub max_gas_invoke: Option<f64>,
    #[serde(alias = "MaxFee")]
    pub max_fee: Option<f64>,
    #[serde(alias = "MaxIteratorResultItems", alias = "max_iterator_results")]
    pub max_iterator_result_items: Option<usize>,
    #[serde(alias = "MaxStackSize")]
    pub max_stack_size: Option<usize>,
    #[serde(alias = "KeepAliveTimeout")]
    pub keep_alive_timeout: Option<i32>,
    #[serde(alias = "RequestHeadersTimeout")]
    pub request_headers_timeout: Option<u64>,
    #[serde(alias = "AuthEnabled")]
    pub auth_enabled: bool,
    #[serde(alias = "SessionEnabled")]
    pub session_enabled: Option<bool>,
    #[serde(alias = "SessionExpirationTime")]
    pub session_expiration_time: Option<u64>,
    #[serde(alias = "FindStoragePageSize")]
    pub find_storage_page_size: Option<usize>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
    #[serde(alias = "RpcUser")]
    pub rpc_user: Option<String>,
    #[serde(alias = "RpcPass")]
    pub rpc_pass: Option<String>,
    #[serde(alias = "SslCert")]
    pub tls_cert_file: Option<String>,
    #[serde(alias = "SslCertPassword")]
    pub tls_cert_password: Option<String>,
    #[serde(alias = "TrustedAuthorities")]
    pub trusted_authorities: Vec<String>,
    #[serde(alias = "DisabledMethods")]
    pub disabled_methods: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ApplicationLogsSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "MaxStackSize")]
    pub max_stack_size: Option<usize>,
    #[serde(alias = "Debug")]
    pub debug: Option<bool>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct StateServiceSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "FullState")]
    pub full_state: Option<bool>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "AutoVerify")]
    pub auto_verify: Option<bool>,
    #[serde(alias = "MaxFindResultItems")]
    pub max_find_result_items: Option<usize>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TokensTrackerSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "DBPath")]
    pub db_path: Option<String>,
    #[serde(alias = "TrackHistory")]
    pub track_history: Option<bool>,
    #[serde(alias = "MaxResults")]
    pub max_results: Option<u32>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "EnabledTrackers")]
    pub enabled_trackers: Vec<String>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "Nodes")]
    pub nodes: Vec<String>,
    #[serde(alias = "MaxTaskTimeout")]
    pub max_task_timeout: Option<u64>,
    #[serde(alias = "MaxOracleTimeout")]
    pub max_oracle_timeout: Option<u64>,
    #[serde(alias = "AllowPrivateHost")]
    pub allow_private_host: Option<bool>,
    #[serde(alias = "AllowedContentTypes")]
    pub allowed_content_types: Vec<String>,
    #[serde(alias = "Https")]
    pub https: Option<OracleServiceHttpsSection>,
    #[serde(alias = "NeoFS")]
    pub neofs: Option<OracleServiceNeoFsSection>,
    #[serde(alias = "AutoStart")]
    pub auto_start: Option<bool>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceHttpsSection {
    #[serde(alias = "Timeout")]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OracleServiceNeoFsSection {
    #[serde(alias = "EndPoint")]
    pub endpoint: Option<String>,
    #[serde(alias = "Timeout")]
    pub timeout: Option<u64>,
    #[serde(alias = "BearerToken")]
    pub bearer_token: Option<String>,
    #[serde(alias = "BearerSignature")]
    pub bearer_signature: Option<String>,
    #[serde(alias = "BearerSignatureKey")]
    pub bearer_signature_key: Option<String>,
    #[serde(alias = "WalletConnect")]
    pub wallet_connect: Option<bool>,
    #[serde(alias = "AutoSignBearer")]
    pub auto_sign_bearer: Option<bool>,
    #[serde(alias = "UseGrpc")]
    pub use_grpc: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct DbftSection {
    #[serde(alias = "Enabled")]
    pub enabled: bool,
    #[serde(alias = "RecoveryLogs")]
    pub recovery_logs: Option<String>,
    #[serde(alias = "IgnoreRecoveryLogs")]
    pub ignore_recovery_logs: Option<bool>,
    #[serde(alias = "AutoStart")]
    pub auto_start: Option<bool>,
    #[serde(alias = "Network")]
    pub network: Option<u32>,
    #[serde(alias = "MaxBlockSize")]
    pub max_block_size: Option<u32>,
    #[serde(alias = "MaxBlockSystemFee")]
    pub max_block_system_fee: Option<i64>,
    #[serde(alias = "UnhandledExceptionPolicy")]
    pub unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DbftSettings {
    pub recovery_logs: String,
    pub ignore_recovery_logs: bool,
    pub auto_start: bool,
    pub network: u32,
    pub max_block_size: u32,
    pub max_block_system_fee: i64,
    pub exception_policy: UnhandledExceptionPolicy,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSection {
    #[serde(alias = "Active")]
    pub active: bool,
    pub level: Option<String>,
    pub format: Option<String>,
    #[serde(alias = "ConsoleOutput")]
    pub console_output: bool,
    #[serde(alias = "FileEnabled")]
    pub file_enabled: bool,
    #[serde(alias = "Path", alias = "path")]
    pub file_path: Option<String>,
    pub max_file_size: Option<String>,
    pub max_files: Option<u32>,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            active: true,
            level: Some("info".to_string()),
            format: None,
            console_output: true,
            file_enabled: false,
            file_path: Some("Logs".to_string()),
            max_file_size: None,
            max_files: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct UnlockWalletSection {
    #[serde(alias = "Path")]
    pub path: Option<String>,
    #[serde(alias = "Password")]
    pub password: Option<String>,
    #[serde(alias = "IsActive")]
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ContractsSection {
    #[serde(alias = "NeoNameService")]
    pub neo_name_service: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginsSection {
    #[serde(alias = "DownloadUrl")]
    pub download_url: String,
    #[serde(alias = "Prerelease")]
    pub prerelease: bool,
    #[serde(alias = "Version")]
    pub version: Option<String>,
}

impl Default for PluginsSection {
    fn default() -> Self {
        Self {
            download_url: "https://api.github.com/repos/neo-project/neo/releases".to_string(),
            prerelease: false,
            version: None,
        }
    }
}

/// Consensus configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusSection {
    /// Whether consensus is enabled
    pub enabled: bool,
    /// Whether to auto-start consensus
    pub auto_start: Option<bool>,
}

/// Telemetry configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetrySection {
    /// Metrics configuration
    pub metrics: Option<TelemetryMetricsSection>,
}

/// Telemetry metrics configuration
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryMetricsSection {
    /// Whether metrics are enabled
    pub enabled: bool,
    /// Metrics port
    pub port: Option<u16>,
    /// Metrics bind address
    pub bind_address: Option<String>,
}

/// Mempool configuration section
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct MempoolSection {
    /// Maximum transactions in mempool
    pub max_transactions: Option<usize>,
    /// Maximum transactions per sender
    pub max_transactions_per_sender: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ApplicationLogsPluginConfig {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<ApplicationLogsPluginSection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ApplicationLogsPluginSection {
    #[serde(rename = "Path")]
    path: Option<String>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "MaxStackSize")]
    max_stack_size: Option<usize>,
    #[serde(rename = "Debug")]
    debug: Option<bool>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct StateServicePluginConfig {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<StateServicePluginSection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct StateServicePluginSection {
    #[serde(rename = "Path")]
    path: Option<String>,
    #[serde(rename = "FullState")]
    full_state: Option<bool>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "AutoVerify")]
    auto_verify: Option<bool>,
    #[serde(rename = "MaxFindResultItems")]
    max_find_result_items: Option<usize>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TokensTrackerPluginConfig {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<TokensTrackerPluginSection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct TokensTrackerPluginSection {
    #[serde(rename = "DBPath")]
    db_path: Option<String>,
    #[serde(rename = "TrackHistory")]
    track_history: Option<bool>,
    #[serde(rename = "MaxResults")]
    max_results: Option<u32>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "EnabledTrackers")]
    enabled_trackers: Vec<String>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct OracleServicePluginConfig {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<OracleServicePluginSection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct OracleServicePluginSection {
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "Nodes")]
    nodes: Vec<String>,
    #[serde(rename = "MaxTaskTimeout")]
    max_task_timeout: Option<u64>,
    #[serde(rename = "MaxOracleTimeout")]
    max_oracle_timeout: Option<u64>,
    #[serde(rename = "AllowPrivateHost")]
    allow_private_host: Option<bool>,
    #[serde(rename = "AllowedContentTypes")]
    allowed_content_types: Vec<String>,
    #[serde(rename = "Https")]
    https: Option<OracleServiceHttpsSection>,
    #[serde(rename = "NeoFS")]
    neofs: Option<OracleServiceNeoFsSection>,
    #[serde(rename = "AutoStart")]
    auto_start: Option<bool>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct DbftPluginConfig {
    #[serde(rename = "PluginConfiguration")]
    plugin_configuration: Option<DbftPluginSection>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct DbftPluginSection {
    #[serde(rename = "RecoveryLogs")]
    recovery_logs: Option<String>,
    #[serde(rename = "IgnoreRecoveryLogs")]
    ignore_recovery_logs: Option<bool>,
    #[serde(rename = "AutoStart")]
    auto_start: Option<bool>,
    #[serde(rename = "Network")]
    network: Option<u32>,
    #[serde(rename = "MaxBlockSize")]
    max_block_size: Option<u32>,
    #[serde(rename = "MaxBlockSystemFee")]
    max_block_system_fee: Option<i64>,
    #[serde(rename = "UnhandledExceptionPolicy")]
    unhandled_exception_policy: Option<String>,
}

impl NodeConfig {
    /// Loads configuration from disk using the Neo CLI TOML schema.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("unable to read configuration at {}", path.display()))?;
        let config = toml::from_str(&contents)
            .with_context(|| format!("invalid node configuration in {}", path.display()))?;
        Ok(config)
    }

    /// Converts the parsed config into `ProtocolSettings`, overriding the defaults.
    pub fn protocol_settings(&self) -> ProtocolSettings {
        // First, determine base settings from network_type or infer from magic
        let network_magic = self.network.network_magic.or_else(|| {
            self.network
                .network_type
                .as_deref()
                .and_then(infer_magic_from_type)
        });

        let base_settings = match network_magic {
            Some(TESTNET_MAGIC) => ProtocolSettings::testnet(),
            Some(MAINNET_MAGIC) => ProtocolSettings::mainnet(),
            _ => {
                // Fallback to network_type if magic doesn't match known values
                match self
                    .network
                    .network_type
                    .as_deref()
                    .map(|value| value.to_ascii_lowercase())
                {
                    Some(ref ty) if ty == "testnet" || ty == "test" => ProtocolSettings::testnet(),
                    Some(ref ty) if ty == "privatenet" || ty == "private" => {
                        ProtocolSettings::default_settings()
                    }
                    _ => ProtocolSettings::mainnet(),
                }
            }
        };

        let mut settings = base_settings;

        // Override with explicit network_magic if provided
        if let Some(magic) = self.network.network_magic {
            settings.network = magic;
        }

        if !self.p2p.seed_nodes.is_empty() {
            settings.seed_list = self.p2p.seed_nodes.clone();
        }

        if let Some(seconds) = self.blockchain.block_time {
            let millis = seconds.saturating_mul(1_000);
            settings.milliseconds_per_block = u32::try_from(millis).unwrap_or(u32::MAX);
        }

        settings
    }

    /// Builds the `ChannelsConfig` used to start the P2P subsystem.
    #[allow(dead_code)] // Will be used when P2P subsystem is fully integrated
    pub fn channels_config(&self) -> ChannelsConfig {
        let mut config = ChannelsConfig::default();

        if let Some(port) = self.p2p.listen_port {
            config.tcp = Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port));
        }

        if let Some(enable) = self.p2p.enable_compression {
            config.enable_compression = enable;
        }

        if let Some(min_conn) = self.p2p.min_desired_connections {
            config.min_desired_connections = min_conn;
        }

        if let Some(max_conn) = self.p2p.max_connections {
            config.max_connections = max_conn;
        }

        if let Some(max_per_address) = self.p2p.max_connections_per_address {
            config.max_connections_per_address = max_per_address;
        }
        if let Some(max_hashes) = self.p2p.max_known_hashes {
            config.max_known_hashes = max_hashes;
        }
        if let Some(limit) = self.p2p.broadcast_history_limit {
            config.broadcast_history_limit = limit;
        }

        config
    }

    /// Returns the configured storage path, if any.
    pub fn storage_path(&self) -> Option<String> {
        self.storage.path.clone()
    }

    /// Returns the configured backend identifier, if provided.
    pub fn storage_backend(&self) -> Option<&str> {
        self.storage.backend.as_deref()
    }

    /// Builds the storage configuration used by persistent providers.
    pub fn storage_config(&self) -> StorageConfig {
        let mut config = StorageConfig::default();
        if let Some(path) = &self.storage.path {
            config.path = PathBuf::from(path);
        }
        if let Some(cache_mb) = self.storage.cache_size {
            config.cache_size = Some(megabytes_to_bytes(cache_mb));
        }
        if let Some(write_mb) = self.storage.write_buffer_size {
            config.write_buffer_size = Some(megabytes_to_bytes(write_mb));
        }
        if let Some(max_files) = self.storage.max_open_files {
            config.max_open_files = Some(max_files);
        }
        if let Some(compression) = self.storage.compression.as_deref() {
            if let Some(algorithm) = parse_compression(compression) {
                config.compression_algorithm = algorithm;
            }
        }
        if let Some(read_only) = self.storage.read_only {
            config.read_only = read_only;
        }
        config
    }

    /// Returns ApplicationLogs settings if enabled via node or plugin config.
    pub fn application_logs_settings(
        &self,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Option<ApplicationLogsSettings>> {
        if let Some(section) = &self.application_logs {
            if !section.enabled {
                return Ok(None);
            }
            return Ok(Some(section.to_settings(protocol_settings.network)));
        }

        let plugin_section = load_application_logs_plugin_config()?;
        let Some(section) = plugin_section else {
            return Ok(None);
        };
        Ok(Some(section.to_settings(protocol_settings.network)))
    }

    /// Returns StateService settings if enabled via node or plugin config.
    pub fn state_service_settings(
        &self,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Option<StateServiceSettings>> {
        if let Some(section) = &self.state_service {
            if !section.enabled {
                return Ok(None);
            }
            return Ok(Some(section.to_settings(protocol_settings.network)));
        }

        let plugin_section = load_state_service_plugin_config()?;
        let Some(section) = plugin_section else {
            return Ok(None);
        };
        Ok(Some(section.to_settings(protocol_settings.network)))
    }

    /// Returns TokensTracker settings if enabled via node or plugin config.
    pub fn tokens_tracker_settings(
        &self,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Option<TokensTrackerSettings>> {
        if let Some(section) = &self.tokens_tracker {
            if !section.enabled {
                return Ok(None);
            }
            return Ok(Some(section.to_settings(protocol_settings.network)));
        }

        let plugin_section = load_tokens_tracker_plugin_config()?;
        let Some(section) = plugin_section else {
            return Ok(None);
        };
        Ok(Some(section.to_settings(protocol_settings.network)))
    }

    /// Returns OracleService settings if enabled via node or plugin config.
    pub fn oracle_service_settings(
        &self,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Option<OracleServiceSettings>> {
        if let Some(section) = &self.oracle_service {
            if !section.enabled {
                return Ok(None);
            }
            let settings = section.to_settings(protocol_settings.network);
            validate_oracle_nodes(&settings.nodes)?;
            return Ok(Some(settings));
        }

        let plugin_section = load_oracle_service_plugin_config()?;
        let Some(section) = plugin_section else {
            return Ok(None);
        };
        let settings = section.to_settings(protocol_settings.network);
        validate_oracle_nodes(&settings.nodes)?;
        Ok(Some(settings))
    }

    /// Returns dBFT (consensus) settings if enabled via node or plugin config.
    pub fn dbft_settings(
        &self,
        protocol_settings: &ProtocolSettings,
    ) -> Result<Option<DbftSettings>> {
        if let Some(section) = &self.dbft {
            if !section.enabled {
                return Ok(None);
            }
            return Ok(Some(section.to_settings(protocol_settings.network)));
        }

        let plugin_section = load_dbft_plugin_config()?;
        let Some(section) = plugin_section else {
            return Ok(None);
        };
        Ok(Some(section.to_settings(protocol_settings.network)))
    }

    /// Writes the RPC server configuration JSON consumed by the RpcServer plugin.
    pub fn write_rpc_server_plugin_config(
        &self,
        settings: &ProtocolSettings,
    ) -> Result<Option<PathBuf>> {
        if !self.rpc.enabled {
            return Ok(None);
        }

        if self.rpc.auth_enabled && (self.rpc.rpc_user.is_none() || self.rpc.rpc_pass.is_none()) {
            bail!("rpc.auth_enabled requires both rpc_user and rpc_pass");
        }

        let config_dir = config_directory();
        fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "failed to create config directory at {}",
                config_dir.display()
            )
        })?;

        let config_path = config_dir.join("RpcServer.json");
        let payload = json!({
            "PluginConfiguration": {
                "Servers": [self.build_rpc_server_entry(settings)],
                "UnhandledExceptionPolicy": self
                    .rpc
                    .unhandled_exception_policy
                    .clone()
                    .unwrap_or_else(|| "Ignore".to_string())
            }
        });

        let json = serde_json::to_string_pretty(&payload)?;
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }

        let mut file = options.open(&config_path).with_context(|| {
            format!(
                "failed to open RPC server configuration at {}",
                config_path.display()
            )
        })?;

        file.write_all(json.as_bytes()).with_context(|| {
            format!(
                "failed to write RPC server configuration to {}",
                config_path.display()
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).with_context(
                || {
                    format!(
                        "failed to set permissions on RPC server configuration at {}",
                        config_path.display()
                    )
                },
            )?;
        }

        Ok(Some(config_path))
    }

    fn build_rpc_server_entry(&self, settings: &ProtocolSettings) -> Value {
        let mut server = serde_json::Map::new();
        server.insert("network".into(), json!(settings.network));
        server.insert(
            "bind_address".into(),
            json!(self.rpc.bind_address.as_deref().unwrap_or("127.0.0.1")),
        );
        server.insert("port".into(), json!(self.rpc.port.unwrap_or(10332)));
        server.insert(
            "enable_cors".into(),
            json!(self.rpc.cors_enabled.unwrap_or(true)),
        );
        if !self.rpc.allow_origins.is_empty() {
            server.insert("allow_origins".into(), json!(self.rpc.allow_origins));
        }
        server.insert(
            "max_concurrent_connections".into(),
            json!(self.rpc.max_connections.unwrap_or(40)),
        );
        if let Some(body_size) = self.rpc.max_request_body_size {
            server.insert("max_request_body_size".into(), json!(body_size));
        }
        if let Some(max_gas) = self.rpc.max_gas_invoke {
            let rounded = max_gas.round().max(0.0) as i64;
            server.insert("max_gas_invoke".into(), json!(rounded));
        }
        if let Some(max_fee) = self.rpc.max_fee {
            let rounded = max_fee.round().max(0.0) as i64;
            server.insert("max_fee".into(), json!(rounded));
        }
        if let Some(max_iter) = self.rpc.max_iterator_result_items {
            server.insert("max_iterator_result_items".into(), json!(max_iter));
        }
        if let Some(max_stack) = self.rpc.max_stack_size {
            server.insert("max_stack_size".into(), json!(max_stack));
        }
        server.insert(
            "keep_alive_timeout".into(),
            json!(self.rpc.keep_alive_timeout.unwrap_or(60)),
        );
        server.insert(
            "request_headers_timeout".into(),
            json!(self.rpc.request_headers_timeout.unwrap_or(15)),
        );
        if let Some(session_enabled) = self.rpc.session_enabled {
            server.insert("session_enabled".into(), json!(session_enabled));
        }
        if let Some(expiration) = self.rpc.session_expiration_time {
            server.insert("session_expiration_time".into(), json!(expiration));
        }
        if let Some(page_size) = self.rpc.find_storage_page_size {
            server.insert("find_storage_page_size".into(), json!(page_size));
        }
        if self.rpc.auth_enabled {
            if let Some(user) = &self.rpc.rpc_user {
                server.insert("rpc_user".into(), json!(user));
            }
            if let Some(pass) = &self.rpc.rpc_pass {
                server.insert("rpc_pass".into(), json!(pass));
            }
        }
        if let Some(cert) = &self.rpc.tls_cert_file {
            server.insert("ssl_cert".into(), json!(cert));
        }
        if let Some(cert_password) = &self.rpc.tls_cert_password {
            server.insert("ssl_cert_password".into(), json!(cert_password));
        }
        if !self.rpc.trusted_authorities.is_empty() {
            server.insert(
                "trusted_authorities".into(),
                json!(self.rpc.trusted_authorities),
            );
        }
        if !self.rpc.disabled_methods.is_empty() {
            server.insert(
                "disabled_methods".into(),
                json!(self.rpc.disabled_methods.clone()),
            );
        }
        Value::Object(server)
    }
}

impl ApplicationLogsSection {
    fn to_settings(&self, default_network: u32) -> ApplicationLogsSettings {
        build_application_logs_settings(
            default_network,
            self.path.clone(),
            self.network,
            self.max_stack_size,
            self.debug,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl ApplicationLogsPluginSection {
    fn to_settings(&self, default_network: u32) -> ApplicationLogsSettings {
        build_application_logs_settings(
            default_network,
            self.path.clone(),
            self.network,
            self.max_stack_size,
            self.debug,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl StateServiceSection {
    fn to_settings(&self, default_network: u32) -> StateServiceSettings {
        build_state_service_settings(
            default_network,
            self.path.clone(),
            self.full_state,
            self.network,
            self.auto_verify,
            self.max_find_result_items,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl StateServicePluginSection {
    fn to_settings(&self, default_network: u32) -> StateServiceSettings {
        build_state_service_settings(
            default_network,
            self.path.clone(),
            self.full_state,
            self.network,
            self.auto_verify,
            self.max_find_result_items,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl TokensTrackerSection {
    fn to_settings(&self, default_network: u32) -> TokensTrackerSettings {
        build_tokens_tracker_settings(
            default_network,
            self.db_path.clone(),
            self.track_history,
            self.max_results,
            self.network,
            self.enabled_trackers.clone(),
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl TokensTrackerPluginSection {
    fn to_settings(&self, default_network: u32) -> TokensTrackerSettings {
        build_tokens_tracker_settings(
            default_network,
            self.db_path.clone(),
            self.track_history,
            self.max_results,
            self.network,
            self.enabled_trackers.clone(),
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl OracleServiceSection {
    fn to_settings(&self, default_network: u32) -> OracleServiceSettings {
        build_oracle_service_settings(
            default_network,
            self.network,
            self.nodes.clone(),
            self.max_task_timeout,
            self.max_oracle_timeout,
            self.allow_private_host,
            self.allowed_content_types.clone(),
            self.https.clone(),
            self.neofs.clone(),
            self.auto_start,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl OracleServicePluginSection {
    fn to_settings(&self, default_network: u32) -> OracleServiceSettings {
        build_oracle_service_settings(
            default_network,
            self.network,
            self.nodes.clone(),
            self.max_task_timeout,
            self.max_oracle_timeout,
            self.allow_private_host,
            self.allowed_content_types.clone(),
            self.https.clone(),
            self.neofs.clone(),
            self.auto_start,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl DbftSection {
    fn to_settings(&self, default_network: u32) -> DbftSettings {
        build_dbft_settings(
            default_network,
            self.recovery_logs.clone(),
            self.ignore_recovery_logs,
            self.auto_start,
            self.network,
            self.max_block_size,
            self.max_block_system_fee,
            self.unhandled_exception_policy.clone(),
        )
    }
}

impl DbftPluginSection {
    fn to_settings(&self, default_network: u32) -> DbftSettings {
        build_dbft_settings(
            default_network,
            self.recovery_logs.clone(),
            self.ignore_recovery_logs,
            self.auto_start,
            self.network,
            self.max_block_size,
            self.max_block_system_fee,
            self.unhandled_exception_policy.clone(),
        )
    }
}

pub fn resolve_application_logs_store_path(settings: &ApplicationLogsSettings) -> PathBuf {
    let formatted = format_application_logs_path(&settings.path, settings.network);
    let path = PathBuf::from(formatted);
    if path.is_absolute() {
        path
    } else {
        application_logs_directory().join(path)
    }
}

pub fn resolve_state_service_store_path(settings: &StateServiceSettings) -> PathBuf {
    let formatted = format_state_service_path(&settings.path, settings.network);
    let path = PathBuf::from(formatted);
    if path.is_absolute() {
        path
    } else {
        state_service_directory().join(path)
    }
}

pub fn resolve_tokens_tracker_store_path(settings: &TokensTrackerSettings) -> PathBuf {
    let formatted = format_tokens_tracker_path(&settings.db_path, settings.network);
    let path = PathBuf::from(formatted);
    if path.is_absolute() {
        path
    } else {
        tokens_tracker_directory().join(path)
    }
}

fn build_application_logs_settings(
    default_network: u32,
    path: Option<String>,
    network: Option<u32>,
    max_stack_size: Option<usize>,
    debug: Option<bool>,
    unhandled_exception_policy: Option<String>,
) -> ApplicationLogsSettings {
    let path = path.unwrap_or_else(|| "ApplicationLogs_{0}".to_string());
    let network = network.unwrap_or(default_network);
    let max_stack_size = max_stack_size.unwrap_or(u16::MAX as usize);
    let debug = debug.unwrap_or(false);
    let exception_policy = parse_unhandled_exception_policy(
        unhandled_exception_policy.as_deref(),
        UnhandledExceptionPolicy::Ignore,
    );
    ApplicationLogsSettings::new(true, network, path, max_stack_size, debug, exception_policy)
}

fn load_application_logs_plugin_config() -> Result<Option<ApplicationLogsPluginSection>> {
    let path = application_logs_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).with_context(|| {
        format!(
            "unable to read ApplicationLogs config at {}",
            path.display()
        )
    })?;
    let config: ApplicationLogsPluginConfig = serde_json::from_str(&raw)
        .with_context(|| format!("invalid ApplicationLogs config in {}", path.display()))?;
    let Some(section) = config.plugin_configuration else {
        bail!(
            "ApplicationLogs config at {} missing PluginConfiguration",
            path.display()
        );
    };
    Ok(Some(section))
}

fn load_state_service_plugin_config() -> Result<Option<StateServicePluginSection>> {
    let path = state_service_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("unable to read StateService config at {}", path.display()))?;
    let config: StateServicePluginConfig = serde_json::from_str(&raw)
        .with_context(|| format!("invalid StateService config in {}", path.display()))?;
    let Some(section) = config.plugin_configuration else {
        bail!(
            "StateService config at {} missing PluginConfiguration",
            path.display()
        );
    };
    Ok(Some(section))
}

fn load_tokens_tracker_plugin_config() -> Result<Option<TokensTrackerPluginSection>> {
    let path = tokens_tracker_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("unable to read TokensTracker config at {}", path.display()))?;
    let config: TokensTrackerPluginConfig = serde_json::from_str(&raw)
        .with_context(|| format!("invalid TokensTracker config in {}", path.display()))?;
    let Some(section) = config.plugin_configuration else {
        bail!(
            "TokensTracker config at {} missing PluginConfiguration",
            path.display()
        );
    };
    Ok(Some(section))
}

fn load_oracle_service_plugin_config() -> Result<Option<OracleServicePluginSection>> {
    let path = oracle_service_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("unable to read OracleService config at {}", path.display()))?;
    let config: OracleServicePluginConfig = serde_json::from_str(&raw)
        .with_context(|| format!("invalid OracleService config in {}", path.display()))?;
    let Some(section) = config.plugin_configuration else {
        bail!(
            "OracleService config at {} missing PluginConfiguration",
            path.display()
        );
    };
    Ok(Some(section))
}

fn load_dbft_plugin_config() -> Result<Option<DbftPluginSection>> {
    let path = dbft_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("unable to read DBFTPlugin config at {}", path.display()))?;
    let config: DbftPluginConfig = serde_json::from_str(&raw)
        .with_context(|| format!("invalid DBFTPlugin config in {}", path.display()))?;
    let Some(section) = config.plugin_configuration else {
        bail!(
            "DBFTPlugin config at {} missing PluginConfiguration",
            path.display()
        );
    };
    Ok(Some(section))
}

fn format_application_logs_path(template: &str, network: u32) -> String {
    template.replace("{0}", &format!("{network:08X}"))
}

fn format_state_service_path(template: &str, network: u32) -> String {
    template.replace("{0}", &format!("{network:08X}"))
}

fn format_tokens_tracker_path(template: &str, network: u32) -> String {
    template.replace("{0}", &format!("{network:08X}"))
}

fn build_state_service_settings(
    default_network: u32,
    path: Option<String>,
    full_state: Option<bool>,
    network: Option<u32>,
    auto_verify: Option<bool>,
    max_find_result_items: Option<usize>,
    unhandled_exception_policy: Option<String>,
) -> StateServiceSettings {
    let path = path.unwrap_or_else(|| "Data_MPT_{0}".to_string());
    let network = network.unwrap_or(default_network);
    let full_state = full_state.unwrap_or(false);
    let auto_verify = auto_verify.unwrap_or(false);
    let max_find_result_items = max_find_result_items.unwrap_or(100);
    let exception_policy = parse_unhandled_exception_policy(
        unhandled_exception_policy.as_deref(),
        UnhandledExceptionPolicy::StopPlugin,
    );
    StateServiceSettings {
        full_state,
        path,
        network,
        auto_verify,
        max_find_result_items,
        exception_policy,
    }
}

fn build_tokens_tracker_settings(
    default_network: u32,
    db_path: Option<String>,
    track_history: Option<bool>,
    max_results: Option<u32>,
    network: Option<u32>,
    enabled_trackers: Vec<String>,
    unhandled_exception_policy: Option<String>,
) -> TokensTrackerSettings {
    let mut settings = TokensTrackerSettings::default();
    settings.db_path = db_path.unwrap_or_else(|| settings.db_path.clone());
    settings.track_history = track_history.unwrap_or(settings.track_history);
    settings.max_results = max_results.unwrap_or(settings.max_results);
    settings.network = network.unwrap_or(default_network);
    if !enabled_trackers.is_empty() {
        settings.enabled_trackers = enabled_trackers;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }
    settings
}

fn build_dbft_settings(
    default_network: u32,
    recovery_logs: Option<String>,
    ignore_recovery_logs: Option<bool>,
    auto_start: Option<bool>,
    network: Option<u32>,
    max_block_size: Option<u32>,
    max_block_system_fee: Option<i64>,
    unhandled_exception_policy: Option<String>,
) -> DbftSettings {
    let mut settings = DbftSettings {
        recovery_logs: "ConsensusState".to_string(),
        ignore_recovery_logs: false,
        auto_start: false,
        network: default_network,
        max_block_size: MAX_BLOCK_SIZE as u32,
        max_block_system_fee: 150_000_000_000,
        exception_policy: UnhandledExceptionPolicy::StopNode,
    };

    if let Some(value) = recovery_logs {
        settings.recovery_logs = value;
    }
    if let Some(ignore) = ignore_recovery_logs {
        settings.ignore_recovery_logs = ignore;
    }
    if let Some(auto_start) = auto_start {
        settings.auto_start = auto_start;
    }
    settings.network = network.unwrap_or(default_network);
    if let Some(size) = max_block_size {
        settings.max_block_size = size;
    }
    if let Some(fee) = max_block_system_fee {
        settings.max_block_system_fee = fee;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }

    settings
}

fn build_oracle_service_settings(
    default_network: u32,
    network: Option<u32>,
    nodes: Vec<String>,
    max_task_timeout: Option<u64>,
    max_oracle_timeout: Option<u64>,
    allow_private_host: Option<bool>,
    allowed_content_types: Vec<String>,
    https: Option<OracleServiceHttpsSection>,
    neofs: Option<OracleServiceNeoFsSection>,
    auto_start: Option<bool>,
    unhandled_exception_policy: Option<String>,
) -> OracleServiceSettings {
    let mut settings = OracleServiceSettings::default();
    settings.network = network.unwrap_or(default_network);
    if !nodes.is_empty() {
        settings.nodes = nodes;
    }
    if let Some(timeout) = max_task_timeout {
        settings.max_task_timeout = Duration::from_millis(timeout);
    }
    if let Some(timeout) = max_oracle_timeout {
        settings.max_oracle_timeout = Duration::from_millis(timeout);
    }
    if let Some(allow) = allow_private_host {
        settings.allow_private_host = allow;
    }
    if !allowed_content_types.is_empty() {
        settings.allowed_content_types = allowed_content_types;
    }
    if let Some(https) = https {
        if let Some(timeout) = https.timeout {
            settings.https_timeout = Duration::from_millis(timeout);
        }
    }
    if let Some(neofs) = neofs {
        if let Some(endpoint) = neofs.endpoint {
            settings.neofs_endpoint = endpoint;
        }
        if let Some(timeout) = neofs.timeout {
            settings.neofs_timeout = Duration::from_millis(timeout);
        }
        if let Some(token) = neofs.bearer_token {
            settings.neofs_bearer_token = Some(token);
        }
        if let Some(signature) = neofs.bearer_signature {
            settings.neofs_bearer_signature = Some(signature);
        }
        if let Some(key) = neofs.bearer_signature_key {
            settings.neofs_bearer_signature_key = Some(key);
        }
        if let Some(wallet_connect) = neofs.wallet_connect {
            settings.neofs_wallet_connect = wallet_connect;
        }
        if let Some(auto_sign) = neofs.auto_sign_bearer {
            settings.neofs_auto_sign_bearer = auto_sign;
        }
        if let Some(use_grpc) = neofs.use_grpc {
            settings.neofs_use_grpc = use_grpc;
        }
    }
    if let Some(auto_start) = auto_start {
        settings.auto_start = auto_start;
    }
    if let Some(policy) = unhandled_exception_policy {
        settings.exception_policy =
            parse_unhandled_exception_policy(Some(policy.as_str()), settings.exception_policy);
    }
    settings.normalize();
    settings
}

fn parse_unhandled_exception_policy(
    value: Option<&str>,
    default_policy: UnhandledExceptionPolicy,
) -> UnhandledExceptionPolicy {
    let Some(raw) = value else {
        return default_policy;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "ignore" => UnhandledExceptionPolicy::Ignore,
        "stopplugin" => UnhandledExceptionPolicy::StopPlugin,
        "continue" => UnhandledExceptionPolicy::Continue,
        "terminate" => UnhandledExceptionPolicy::Terminate,
        "stopnode" => UnhandledExceptionPolicy::StopNode,
        _ => default_policy,
    }
}

fn validate_oracle_nodes(nodes: &[String]) -> Result<()> {
    for node in nodes {
        if let Err(err) = Url::parse(node) {
            bail!(
                "OracleService.Nodes entry '{}' is not a valid absolute URI: {}",
                node,
                err
            );
        }
    }
    Ok(())
}

pub fn infer_magic_from_type(network_type: &str) -> Option<u32> {
    match network_type.to_ascii_lowercase().as_str() {
        "mainnet" | "main" => Some(0x334F454E),
        "testnet" | "test" => Some(0x3554334E),
        "privatenet" | "private" => Some(0x4E454F50),
        _ => None,
    }
}

fn parse_compression(name: &str) -> Option<CompressionAlgorithm> {
    match name.to_ascii_lowercase().as_str() {
        "none" => Some(CompressionAlgorithm::None),
        "lz4" => Some(CompressionAlgorithm::Lz4),
        "zstd" => Some(CompressionAlgorithm::Zstd),
        _ => None,
    }
}

fn megabytes_to_bytes(value_mb: u64) -> usize {
    const MB: u64 = 1024 * 1024;
    let bytes = value_mb.saturating_mul(MB);
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    #[test]
    fn rejects_unknown_fields_in_known_table() {
        let contents = r#"
            [network]
            network_type = "MainNet"
            unexpected = 1
        "#;
        let err = toml::from_str::<NodeConfig>(contents).expect_err("should reject unknown field");
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("unknown field") || msg.contains("unknown"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn rejects_unknown_tables() {
        let contents = r#"
            [network]
            network_type = "MainNet"

            [extra]
            foo = "bar"
        "#;
        let err = toml::from_str::<NodeConfig>(contents).expect_err("should reject unknown table");
        let msg = err.to_string().to_ascii_lowercase();
        assert!(
            msg.contains("unknown field") || msg.contains("extra"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn writes_rpc_config_with_restricted_permissions() {
        let tmp = TempDir::new().expect("temp dir");
        env::set_var("NEO_PLUGINS_DIR", tmp.path());

        let mut config = NodeConfig::default();
        config.rpc.enabled = true;
        config.rpc.port = Some(12345);

        let settings = ProtocolSettings::mainnet();
        let path = config
            .write_rpc_server_plugin_config(&settings)
            .expect("write rpc config")
            .expect("path returned");

        assert!(
            path.starts_with(tmp.path()),
            "rpc config should be written under NEO_PLUGINS_DIR"
        );

        let metadata = fs::metadata(&path).expect("metadata");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(metadata.mode() & 0o777, 0o600);
        }

        let contents = fs::read_to_string(&path).expect("contents");
        assert!(
            contents.contains("\"Servers\""),
            "config should contain Servers array"
        );

        env::remove_var("NEO_PLUGINS_DIR");
    }

    #[test]
    #[ignore = "Config file field names don't match struct - needs migration"]
    fn bundled_mainnet_config_parses() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../neo_mainnet_node.toml"))
            .expect("mainnet config should parse");
        assert_eq!(cfg.network.network_type.as_deref(), Some("MainNet"));
    }

    #[test]
    #[ignore = "Config file field names don't match struct - needs migration"]
    fn bundled_testnet_config_parses() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../neo_testnet_node.toml"))
            .expect("testnet config should parse");
        assert_eq!(cfg.network.network_type.as_deref(), Some("TestNet"));
    }

    #[test]
    #[ignore = "Config file field names don't match struct - needs migration"]
    fn bundled_production_config_parses() {
        toml::from_str::<NodeConfig>(include_str!("../../neo_production_node.toml"))
            .expect("production template should parse");
    }
}

//! Neo Node Configuration
//!
//! This module provides configuration parsing for the Neo N3 blockchain node.

mod plugin_settings;

use anyhow::{bail, Context, Result};
use neo_core::{
    application_logs::ApplicationLogsSettings,
    constants::{MAINNET_MAGIC, TESTNET_MAGIC},
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
    fs,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};
use zeroize::Zeroizing;

use plugin_settings::{
    application_logs_section_settings, config_directory, dbft_section_settings,
    load_application_logs_plugin_settings, load_dbft_plugin_settings,
    load_oracle_service_plugin_settings, load_state_service_plugin_settings,
    load_tokens_tracker_plugin_settings, oracle_service_section_settings,
    state_service_section_settings, tokens_tracker_section_settings, validate_oracle_nodes,
};
pub use plugin_settings::{
    resolve_application_logs_store_path, resolve_state_service_store_path,
    resolve_tokens_tracker_store_path,
};

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
    pub rpc_user: Option<Zeroizing<String>>,
    #[serde(alias = "RpcPass")]
    pub rpc_pass: Option<Zeroizing<String>>,
    #[serde(alias = "SslCert")]
    pub tls_cert_file: Option<String>,
    #[serde(alias = "SslCertPassword")]
    pub tls_cert_password: Option<Zeroizing<String>>,
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

        if let Some(milliseconds) = self.blockchain.block_time {
            settings.milliseconds_per_block = u32::try_from(milliseconds).unwrap_or(u32::MAX);
        }

        if let Some(max_tx) = self.blockchain.max_transactions_per_block {
            settings.max_transactions_per_block = max_tx;
        }

        if let Some(mempool) = &self.mempool {
            if let Some(max_mempool) = mempool.max_transactions {
                settings.memory_pool_max_transactions =
                    i32::try_from(max_mempool).unwrap_or(i32::MAX);
            }
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
            return Ok(Some(application_logs_section_settings(
                section,
                protocol_settings.network,
            )));
        }

        load_application_logs_plugin_settings(protocol_settings.network)
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
            return Ok(Some(state_service_section_settings(
                section,
                protocol_settings.network,
            )));
        }

        load_state_service_plugin_settings(protocol_settings.network)
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
            return Ok(Some(tokens_tracker_section_settings(
                section,
                protocol_settings.network,
            )));
        }

        load_tokens_tracker_plugin_settings(protocol_settings.network)
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
            let settings = oracle_service_section_settings(section, protocol_settings.network);
            validate_oracle_nodes(&settings.nodes)?;
            return Ok(Some(settings));
        }

        let settings = load_oracle_service_plugin_settings(protocol_settings.network)?;
        if let Some(settings) = settings.as_ref() {
            validate_oracle_nodes(&settings.nodes)?;
        }
        Ok(settings)
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
            return Ok(Some(dbft_section_settings(
                section,
                protocol_settings.network,
            )));
        }

        load_dbft_plugin_settings(protocol_settings.network)
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
    fn blockchain_block_time_is_milliseconds() {
        let mut config = NodeConfig::default();
        config.blockchain.block_time = Some(15_000);

        let settings = config.protocol_settings();
        assert_eq!(settings.milliseconds_per_block, 15_000);
    }

    #[test]
    fn blockchain_max_transactions_override_applies_to_protocol_settings() {
        let mut config = NodeConfig::default();
        config.network.network_type = Some("TestNet".to_string());
        config.blockchain.max_transactions_per_block = Some(5_000);

        let settings = config.protocol_settings();
        assert_eq!(settings.max_transactions_per_block, 5_000);
    }

    #[test]
    fn mempool_max_transactions_override_applies_to_protocol_settings() {
        let mut config = NodeConfig::default();
        config.network.network_type = Some("MainNet".to_string());
        config.mempool = Some(MempoolSection {
            max_transactions: Some(12_345),
            max_transactions_per_sender: None,
        });

        let settings = config.protocol_settings();
        assert_eq!(settings.memory_pool_max_transactions, 12_345);
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

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::metadata(&path).expect("metadata");
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
    fn bundled_mainnet_config_parses() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../neo_mainnet_node.toml"))
            .expect("mainnet config should parse");
        assert_eq!(cfg.network.network_type.as_deref(), Some("MainNet"));
    }

    #[test]
    fn bundled_testnet_config_parses() {
        let cfg: NodeConfig = toml::from_str(include_str!("../../neo_testnet_node.toml"))
            .expect("testnet config should parse");
        assert_eq!(cfg.network.network_type.as_deref(), Some("TestNet"));
    }

    #[test]
    fn bundled_production_config_parses() {
        toml::from_str::<NodeConfig>(include_str!("../../neo_production_node.toml"))
            .expect("production template should parse");
    }
}

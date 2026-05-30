//! NodeConfig implementation and utility functions.
//!
//! This module contains the impl block for NodeConfig (load, save, protocol_settings, etc.)
//! along with free utility functions.

use anyhow::{bail, Context, Result};
use neo_application_logs::ApplicationLogsSettings;
use neo_tokens_tracker::TokensTrackerSettings;
use neo_oracle_service::OracleServiceSettings;
use neo_core::{
    constants::{MAINNET_MAGIC, TESTNET_MAGIC},
    network::p2p::channels_config::ChannelsConfig,
    persistence::storage::{CompressionAlgorithm, StorageConfig},
    protocol_settings::ProtocolSettings,
    state_service::state_store::StateServiceSettings,
};
use serde_json::{json, Value};
use std::{
    fs,
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use super::plugin_settings::{
    application_logs_section_settings, config_directory, dbft_section_settings,
    load_application_logs_plugin_settings, load_dbft_plugin_settings,
    load_oracle_service_plugin_settings, load_state_service_plugin_settings,
    load_tokens_tracker_plugin_settings, oracle_service_section_settings,
    state_service_section_settings, tokens_tracker_section_settings, validate_oracle_nodes,
};
use super::sections::*;

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

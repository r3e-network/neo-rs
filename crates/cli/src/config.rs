use anyhow::{Context, Result};
use neo_core::{
    network::p2p::channels_config::ChannelsConfig,
    persistence::storage::{CompressionAlgorithm, StorageConfig},
    protocol_settings::ProtocolSettings,
};
use neo_extensions::plugin::plugins_directory;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

/// High-level node configuration derived from the Neo CLI TOML files.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct NodeConfig {
    pub network: NetworkSection,
    pub p2p: P2PSection,
    pub storage: StorageSection,
    pub blockchain: BlockchainSection,
    pub rpc: RpcSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct NetworkSection {
    pub network_type: Option<String>,
    pub network_magic: Option<u32>,
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            network_type: None,
            network_magic: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct P2PSection {
    pub listen_port: Option<u16>,
    pub min_desired_connections: Option<usize>,
    pub max_connections: Option<usize>,
    pub max_connections_per_address: Option<usize>,
    pub enable_compression: Option<bool>,
    pub seed_nodes: Vec<String>,
}

impl Default for P2PSection {
    fn default() -> Self {
        Self {
            listen_port: None,
            min_desired_connections: None,
            max_connections: None,
            max_connections_per_address: None,
            enable_compression: None,
            seed_nodes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct StorageSection {
    pub path: Option<String>,
    pub backend: Option<String>,
    pub cache_size: Option<u64>,
    pub compression: Option<String>,
    pub write_buffer_size: Option<u64>,
    pub max_open_files: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct BlockchainSection {
    pub block_time: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default, Serialize)]
#[serde(default)]
pub struct RpcSection {
    pub enabled: bool,
    pub bind_address: Option<String>,
    pub port: Option<u16>,
    pub cors_enabled: Option<bool>,
    pub allow_origins: Vec<String>,
    pub max_connections: Option<usize>,
    pub max_request_body_size: Option<usize>,
    pub max_gas_invoke: Option<f64>,
    pub max_fee: Option<f64>,
    pub max_iterator_result_items: Option<usize>,
    pub max_stack_size: Option<usize>,
    pub keep_alive_timeout: Option<i32>,
    pub request_headers_timeout: Option<u64>,
    pub session_enabled: Option<bool>,
    pub session_expiration_time: Option<u64>,
    pub find_storage_page_size: Option<usize>,
    pub unhandled_exception_policy: Option<String>,
    pub rpc_user: Option<String>,
    pub rpc_pass: Option<String>,
    pub tls_cert_file: Option<String>,
    pub tls_cert_password: Option<String>,
    pub trusted_authorities: Vec<String>,
    pub disabled_methods: Vec<String>,
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
        let mut settings = ProtocolSettings::default();

        if let Some(magic) = self.network.network_magic.or_else(|| {
            self.network
                .network_type
                .as_deref()
                .and_then(infer_magic_from_type)
        }) {
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
        config
    }

    /// Writes the RPC server configuration JSON consumed by the RpcServer plugin.
    pub fn write_rpc_server_plugin_config(
        &self,
        settings: &ProtocolSettings,
    ) -> Result<Option<PathBuf>> {
        if !self.rpc.enabled {
            return Ok(None);
        }

        let plugins_dir = plugins_directory();
        fs::create_dir_all(&plugins_dir).with_context(|| {
            format!(
                "failed to create plugins directory at {}",
                plugins_dir.display()
            )
        })?;

        let config_path = plugins_dir.join("RpcServer.json");
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

        fs::write(&config_path, serde_json::to_string_pretty(&payload)?).with_context(|| {
            format!(
                "failed to write RPC server configuration to {}",
                config_path.display()
            )
        })?;

        Ok(Some(config_path))
    }

    fn build_rpc_server_entry(&self, settings: &ProtocolSettings) -> Value {
        let mut server = serde_json::Map::new();
        server.insert("Network".into(), json!(settings.network));
        server.insert(
            "BindAddress".into(),
            json!(self.rpc.bind_address.as_deref().unwrap_or("127.0.0.1")),
        );
        server.insert("Port".into(), json!(self.rpc.port.unwrap_or(10332)));
        server.insert(
            "EnableCors".into(),
            json!(self.rpc.cors_enabled.unwrap_or(true)),
        );
        if !self.rpc.allow_origins.is_empty() {
            server.insert("AllowOrigins".into(), json!(self.rpc.allow_origins));
        }
        server.insert(
            "MaxConcurrentConnections".into(),
            json!(self.rpc.max_connections.unwrap_or(40)),
        );
        if let Some(body_size) = self.rpc.max_request_body_size {
            server.insert("MaxRequestBodySize".into(), json!(body_size));
        }
        if let Some(max_gas) = self.rpc.max_gas_invoke {
            server.insert("MaxGasInvoke".into(), json!(gas_to_datoshi(max_gas)));
        }
        if let Some(max_fee) = self.rpc.max_fee {
            server.insert("MaxFee".into(), json!(gas_to_datoshi(max_fee)));
        }
        if let Some(max_iter) = self.rpc.max_iterator_result_items {
            server.insert("MaxIteratorResultItems".into(), json!(max_iter));
        }
        if let Some(max_stack) = self.rpc.max_stack_size {
            server.insert("MaxStackSize".into(), json!(max_stack));
        }
        server.insert(
            "KeepAliveTimeout".into(),
            json!(self.rpc.keep_alive_timeout.unwrap_or(60)),
        );
        server.insert(
            "RequestHeadersTimeout".into(),
            json!(self.rpc.request_headers_timeout.unwrap_or(15)),
        );
        if let Some(session_enabled) = self.rpc.session_enabled {
            server.insert("SessionEnabled".into(), json!(session_enabled));
        }
        if let Some(expiration) = self.rpc.session_expiration_time {
            server.insert("SessionExpirationTime".into(), json!(expiration));
        }
        if let Some(page_size) = self.rpc.find_storage_page_size {
            server.insert("FindStoragePageSize".into(), json!(page_size));
        }
        if let Some(user) = &self.rpc.rpc_user {
            server.insert("RpcUser".into(), json!(user));
        }
        if let Some(pass) = &self.rpc.rpc_pass {
            server.insert("RpcPass".into(), json!(pass));
        }
        if let Some(cert) = &self.rpc.tls_cert_file {
            server.insert("SslCert".into(), json!(cert));
        }
        if let Some(cert_password) = &self.rpc.tls_cert_password {
            server.insert("SslCertPassword".into(), json!(cert_password));
        }
        if !self.rpc.trusted_authorities.is_empty() {
            server.insert(
                "TrustedAuthorities".into(),
                json!(self.rpc.trusted_authorities),
            );
        }
        if !self.rpc.disabled_methods.is_empty() {
            server.insert(
                "DisabledMethods".into(),
                json!(self.rpc.disabled_methods.clone()),
            );
        }
        Value::Object(server)
    }
}

fn infer_magic_from_type(network_type: &str) -> Option<u32> {
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

fn gas_to_datoshi(amount: f64) -> i64 {
    const FACTOR: f64 = 100_000_000.0;
    (amount * FACTOR) as i64
}

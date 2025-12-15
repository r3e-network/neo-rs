//! Main settings for Neo node configuration

use crate::{ConfigError, ConfigResult, GenesisConfig, NetworkConfig, NetworkType, ProtocolSettings};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Complete node settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Node identification
    #[serde(default)]
    pub node: NodeSettings,

    /// Network configuration
    #[serde(default)]
    pub network: NetworkConfig,

    /// Protocol settings
    #[serde(default)]
    pub protocol: ProtocolSettings,

    /// Genesis configuration
    #[serde(default)]
    pub genesis: GenesisConfig,

    /// Storage settings
    #[serde(default)]
    pub storage: StorageSettings,

    /// RPC server settings
    #[serde(default)]
    pub rpc: RpcSettings,

    /// Consensus settings
    #[serde(default)]
    pub consensus: ConsensusSettings,

    /// Logging settings
    #[serde(default)]
    pub logging: LoggingSettings,

    /// Telemetry settings
    #[serde(default)]
    pub telemetry: TelemetrySettings,
}

/// Node identification and basic settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    /// Node name for identification
    #[serde(default = "default_node_name")]
    pub name: String,

    /// Listen address for P2P connections
    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    /// P2P port
    #[serde(default = "default_p2p_port")]
    pub p2p_port: u16,

    /// User agent string
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSettings {
    /// Path to data directory
    #[serde(default = "default_data_path")]
    pub path: PathBuf,

    /// RocksDB cache size in MB
    #[serde(default = "default_cache_size")]
    pub cache_size_mb: usize,

    /// Maximum open files for RocksDB
    #[serde(default = "default_max_open_files")]
    pub max_open_files: i32,

    /// Enable compression
    #[serde(default = "default_compression")]
    pub compression: bool,
}

/// RPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcSettings {
    /// Enable RPC server
    #[serde(default = "default_rpc_enabled")]
    pub enabled: bool,

    /// RPC listen address
    #[serde(default = "default_rpc_address")]
    pub address: String,

    /// RPC port
    #[serde(default = "default_rpc_port")]
    pub port: u16,

    /// Maximum concurrent requests
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,

    /// Request timeout in seconds
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// Enable session state storage for iterator operations
    #[serde(default)]
    pub session_enabled: bool,

    /// Maximum sessions
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusSettings {
    /// Enable consensus participation
    #[serde(default)]
    pub enabled: bool,

    /// Path to wallet file for consensus
    pub wallet_path: Option<PathBuf>,

    /// Wallet password (should be loaded from secure source in production)
    #[serde(skip_serializing)]
    pub wallet_password: Option<String>,

    /// Consensus timeout multiplier
    #[serde(default = "default_timeout_multiplier")]
    pub timeout_multiplier: f64,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSettings {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Log format (json, text)
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Log file path (None for stdout only)
    pub file: Option<PathBuf>,

    /// Enable color output
    #[serde(default = "default_color_enabled")]
    pub color: bool,
}

/// Telemetry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySettings {
    /// Enable metrics collection
    #[serde(default)]
    pub metrics_enabled: bool,

    /// Metrics endpoint address
    #[serde(default = "default_metrics_address")]
    pub metrics_address: String,

    /// Metrics port
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,

    /// Enable health check endpoint
    #[serde(default = "default_health_enabled")]
    pub health_enabled: bool,
}

// Default value functions
fn default_node_name() -> String {
    format!("neo-rs-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn default_listen_address() -> String {
    "0.0.0.0".to_string()
}

fn default_p2p_port() -> u16 {
    10333
}

fn default_user_agent() -> String {
    format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"))
}

fn default_data_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("neo-rs")
}

fn default_cache_size() -> usize {
    256
}

fn default_max_open_files() -> i32 {
    1000
}

fn default_compression() -> bool {
    true
}

fn default_rpc_enabled() -> bool {
    true
}

fn default_rpc_address() -> String {
    "127.0.0.1".to_string()
}

fn default_rpc_port() -> u16 {
    10332
}

fn default_max_concurrent() -> usize {
    100
}

fn default_request_timeout() -> u64 {
    30
}

fn default_max_sessions() -> usize {
    100
}

fn default_timeout_multiplier() -> f64 {
    1.0
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

fn default_color_enabled() -> bool {
    true
}

fn default_metrics_address() -> String {
    "127.0.0.1".to_string()
}

fn default_metrics_port() -> u16 {
    9090
}

fn default_health_enabled() -> bool {
    true
}

// Default implementations
impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            name: default_node_name(),
            listen_address: default_listen_address(),
            p2p_port: default_p2p_port(),
            user_agent: default_user_agent(),
        }
    }
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            path: default_data_path(),
            cache_size_mb: default_cache_size(),
            max_open_files: default_max_open_files(),
            compression: default_compression(),
        }
    }
}

impl Default for RpcSettings {
    fn default() -> Self {
        Self {
            enabled: default_rpc_enabled(),
            address: default_rpc_address(),
            port: default_rpc_port(),
            max_concurrent_requests: default_max_concurrent(),
            request_timeout_secs: default_request_timeout(),
            session_enabled: false,
            max_sessions: default_max_sessions(),
        }
    }
}

impl Default for ConsensusSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            wallet_path: None,
            wallet_password: None,
            timeout_multiplier: default_timeout_multiplier(),
        }
    }
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            file: None,
            color: default_color_enabled(),
        }
    }
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            metrics_enabled: false,
            metrics_address: default_metrics_address(),
            metrics_port: default_metrics_port(),
            health_enabled: default_health_enabled(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::for_network(NetworkType::MainNet)
    }
}

impl Settings {
    /// Create settings for a specific network
    pub fn for_network(network_type: NetworkType) -> Self {
        let (protocol, genesis, network) = match network_type {
            NetworkType::MainNet => (
                ProtocolSettings::mainnet(),
                GenesisConfig::mainnet(),
                NetworkConfig::for_network(NetworkType::MainNet),
            ),
            NetworkType::TestNet => (
                ProtocolSettings::testnet(),
                GenesisConfig::testnet(),
                NetworkConfig::for_network(NetworkType::TestNet),
            ),
            NetworkType::Private => (
                ProtocolSettings::private(0x01020304),
                GenesisConfig::default(),
                NetworkConfig::for_network(NetworkType::Private),
            ),
        };

        // Update node settings based on network
        let node = NodeSettings {
            p2p_port: match network_type {
                NetworkType::MainNet => 10333,
                NetworkType::TestNet => 20333,
                NetworkType::Private => 30333,
            },
            ..Default::default()
        };

        let rpc = RpcSettings {
            port: match network_type {
                NetworkType::MainNet => 10332,
                NetworkType::TestNet => 20332,
                NetworkType::Private => 30332,
            },
            ..Default::default()
        };

        Self {
            node,
            network,
            protocol,
            genesis,
            storage: StorageSettings::default(),
            rpc,
            consensus: ConsensusSettings::default(),
            logging: LoggingSettings::default(),
            telemetry: TelemetrySettings::default(),
        }
    }

    /// Load settings from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> ConfigResult<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(ConfigError::FileNotFound(path.to_path_buf()));
        }

        let content = std::fs::read_to_string(path)?;
        let settings: Self = toml::from_str(&content)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Load settings from a TOML string
    pub fn from_str(content: &str) -> ConfigResult<Self> {
        let settings: Self = toml::from_str(content)?;
        settings.validate()?;
        Ok(settings)
    }

    /// Save settings to a TOML file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> ConfigResult<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Convert to TOML string
    pub fn to_toml(&self) -> ConfigResult<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Validate settings
    pub fn validate(&self) -> ConfigResult<()> {
        // Validate node settings
        if self.node.p2p_port == 0 {
            return Err(ConfigError::InvalidValue("P2P port cannot be 0".to_string()));
        }

        // Validate RPC settings
        if self.rpc.enabled && self.rpc.port == 0 {
            return Err(ConfigError::InvalidValue("RPC port cannot be 0".to_string()));
        }

        // Validate consensus settings
        if self.consensus.enabled && self.consensus.wallet_path.is_none() {
            return Err(ConfigError::MissingField(
                "consensus.wallet_path is required when consensus is enabled".to_string(),
            ));
        }

        // Validate genesis
        self.genesis.validate()?;

        Ok(())
    }

    /// Get the effective network magic
    pub fn network_magic(&self) -> u32 {
        self.network.effective_magic()
    }

    /// Get the effective address version
    pub fn address_version(&self) -> u8 {
        self.network.effective_address_version()
    }

    /// Get P2P socket address
    pub fn p2p_socket_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.node.listen_address, self.node.p2p_port)
            .parse()
            .expect("Invalid P2P address")
    }

    /// Get RPC socket address
    pub fn rpc_socket_addr(&self) -> std::net::SocketAddr {
        format!("{}:{}", self.rpc.address, self.rpc.port)
            .parse()
            .expect("Invalid RPC address")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert!(settings.validate().is_ok());
        assert_eq!(settings.node.p2p_port, 10333);
    }

    #[test]
    fn test_testnet_settings() {
        let settings = Settings::for_network(NetworkType::TestNet);
        assert_eq!(settings.node.p2p_port, 20333);
        assert_eq!(settings.rpc.port, 20332);
    }

    #[test]
    fn test_toml_roundtrip() {
        let settings = Settings::default();
        let toml = settings.to_toml().unwrap();
        let parsed = Settings::from_str(&toml).unwrap();
        assert_eq!(settings.node.p2p_port, parsed.node.p2p_port);
    }

    #[test]
    fn test_validation() {
        let mut settings = Settings::default();
        settings.node.p2p_port = 0;
        assert!(settings.validate().is_err());
    }
}

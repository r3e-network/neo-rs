//! Configuration Management for Neo-Rust CLI
//!
//! This module handles all configuration including command-line arguments,
//! configuration files, and network settings.

use anyhow::{Context, Result};
use clap::ArgMatches;
use neo_config::{MILLISECONDS_PER_BLOCK, MAX_TRANSACTIONS_PER_BLOCK, MAX_TRACEABLE_BLOCKS, MAX_SCRIPT_SIZE};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::Level;
/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage engine (rocksdb, memory)
    pub engine: String,
    /// Storage path
    pub path: String,
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub bind_port: u16,
    pub public_port: u16,
    pub max_peers: usize,
    pub enable_upnp: bool,
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Storage configuration
    pub storage: StorageConfig,
    /// Network configuration
    pub network: NetworkConfig,
    /// Logger configuration
    pub logger: LoggerConfig,
    /// Wallet configuration
    pub wallet: WalletConfig,
}

/// Logger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggerConfig {
    /// Console output enabled
    pub console_output: bool,
    /// Log level
    pub level: String,
    /// Log file path
    pub file_path: Option<String>,
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Auto-unlock wallet path
    pub path: Option<String>,
    /// Auto-unlock wallet password
    pub password: Option<String>,
    /// Whether auto-unlock is active
    pub is_active: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            storage: StorageConfig::default(),
            network: NetworkConfig::default(),
            logger: LoggerConfig::default(),
            wallet: WalletConfig::default(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            engine: "rocksdb".to_string(),
            path: "chain".to_string(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bind_port: 10332,
            public_port: 10333,
            max_peers: 10,
            enable_upnp: false,
        }
    }
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            console_output: true,
            level: "info".to_string(),
            file_path: None,
        }
    }
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            path: None,
            password: None,
            is_active: false,
        }
    }
}

impl Config {
    /// Load configuration from file and command-line arguments
    pub async fn load(config_path: &PathBuf, args: &CliArgs) -> Result<Self> {
        let mut config = if config_path.exists() {
            let content = tokio::fs::read_to_string(config_path).await?;
            serde_json::from_str(&content)?
        } else {
            Self::default()
        };

        // Override with command-line arguments
        config.apply_args(args);

        Ok(config)
    }

    /// Apply command-line arguments to override configuration
    fn apply_args(&mut self, args: &CliArgs) {
        // Override storage path based on data directory
        self.storage.path = args.data_dir.to_string_lossy().to_string();

        // Override wallet settings
        if let Some(wallet_path) = &args.wallet {
            self.wallet.path = Some(wallet_path.to_string_lossy().to_string());
            self.wallet.is_active = true;
        }

        if let Some(password) = &args.wallet_password {
            self.wallet.password = Some(password.clone());
        }

        // Override log level based on verbosity
        self.logger.level = match args.verbosity {
            0 => "warn".to_string(),
            1 => "info".to_string(),
            2 => "debug".to_string(),
            _ => "trace".to_string(),
        };
    }

    /// Save configuration to file
    pub async fn save(&self, config_path: &PathBuf) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(config_path, content).await?;
        Ok(())
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate storage configuration
        if self.storage.engine.is_empty() {
            anyhow::bail!("Storage engine cannot be empty");
        }

        if self.storage.path.is_empty() {
            anyhow::bail!("Storage path cannot be empty");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.storage.engine, "rocksdb");
        assert!(config.validate().is_ok());
    }

    #[tokio::test]
    async fn test_config_save() {
        let final_dir = TempDir::new().expect("operation should succeed");
        let config_path = final_dir.path().join("config.json");

        let config = Config::default();
        config
            .save(&config_path)
            .await
            .expect("operation should succeed");

        assert!(config_path.exists());

        let content = tokio::fs::read_to_string(&config_path)
            .await
            .expect("operation should succeed");
        let loaded_config: Config =
            serde_json::from_str(&content).expect("Failed to parse from string");
        assert_eq!(loaded_config.storage.path, config.storage.path);
    }

    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        assert!(config.validate().is_ok());

        // Test invalid storage path
        config.storage.path = "".to_string();
        assert!(config.validate().is_err());
    }
}

/// CLI Arguments parsed from command line
#[derive(Debug, Clone)]
pub struct CliArgs {
    pub config: PathBuf,
    pub network: NetworkType,
    pub rpc_enabled: bool,
    pub rpc_port: u16,
    pub p2p_port: u16,
    pub wallet: Option<PathBuf>,
    pub wallet_password: Option<String>,
    pub no_verify: bool,
    pub verbosity: u8,
    pub plugins: Vec<String>,
    pub daemon: bool,
    pub console: bool,
    pub log_level: Level,
    pub data_dir: PathBuf,
    pub subcommand: Option<CliSubcommand>,
}

/// Supported network types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkType {
    Mainnet,
    Testnet,
}

/// CLI subcommands
#[derive(Debug, Clone)]
pub enum CliSubcommand {
    Wallet(WalletCommand),
    Node(NodeCommand),
    Rpc(RpcCommand),
}

/// Wallet subcommands
#[derive(Debug, Clone)]
pub enum WalletCommand {
    Create {
        path: PathBuf,
        password: Option<String>,
    },
    Open {
        path: PathBuf,
        password: Option<String>,
    },
    List,
    Balance,
}

/// Node subcommands
#[derive(Debug, Clone)]
pub enum NodeCommand {
    Start,
    Stop,
    Status,
}

/// RPC subcommands
#[derive(Debug, Clone)]
pub enum RpcCommand {
    Start,
    Stop,
}

impl CliArgs {
    /// Parse CLI arguments from clap matches
    pub fn from_matches(matches: &ArgMatches) -> Result<Self> {
        let config = PathBuf::from(matches.get_one::<String>("config")?);

        let network = if matches.get_flag("testnet") {
            NetworkType::Testnet
        } else {
            NetworkType::Mainnet // Default to mainnet
        };

        let rpc_enabled = matches.get_flag("rpc");
        let rpc_port = matches
            .get_one::<String>("rpc-port")?
            .parse::<u16>()
            .context("Invalid RPC port")?;

        let p2p_port = matches
            .get_one::<String>("p2p-port")?
            .parse::<u16>()
            .context("Invalid P2P port")?;

        let wallet = matches.get_one::<String>("wallet").map(PathBuf::from);

        let wallet_password = matches
            .get_one::<String>("wallet-password")
            .map(String::from);

        let no_verify = matches.get_flag("no-verify");

        let verbosity = matches.get_count("verbose");

        let plugins = matches
            .get_many::<String>("plugins")
            .map(|values| values.map(String::from).collect())
            .unwrap_or_default();

        let daemon = matches.get_flag("daemon");
        let console = matches.get_flag("console");

        let log_level = match matches.get_one::<String>("log-level")?.as_str() {
            "error" => Level::ERROR,
            "warn" => Level::WARN,
            "info" => Level::INFO,
            "debug" => Level::DEBUG,
            "trace" => Level::TRACE,
            _ => Level::INFO,
        };

        let data_dir = PathBuf::from(matches.get_one::<String>("data-dir")?);

        let subcommand = match matches.subcommand() {
            Some(("wallet", wallet_matches)) => {
                Some(CliSubcommand::Wallet(parse_wallet_command(wallet_matches)?))
            }
            Some(("node", node_matches)) => {
                Some(CliSubcommand::Node(parse_node_command(node_matches)?))
            }
            Some(("rpc", rpc_matches)) => Some(CliSubcommand::Rpc(parse_rpc_command(rpc_matches)?)),
            _ => None,
        };

        Ok(Self {
            config,
            network,
            rpc_enabled,
            rpc_port,
            p2p_port,
            wallet,
            wallet_password,
            no_verify,
            verbosity,
            plugins,
            daemon,
            console,
            log_level,
            data_dir,
            subcommand,
        })
    }
}

fn parse_wallet_command(matches: &ArgMatches) -> Result<WalletCommand> {
    match matches.subcommand() {
        Some(("create", create_matches)) => {
            let path = PathBuf::from(create_matches.get_one::<String>("path")?);
            let password = create_matches
                .get_one::<String>("password")
                .map(String::from);
            Ok(WalletCommand::Create { path, password })
        }
        Some(("open", open_matches)) => {
            let path = PathBuf::from(open_matches.get_one::<String>("path")?);
            let password = open_matches.get_one::<String>("password").map(String::from);
            Ok(WalletCommand::Open { path, password })
        }
        Some(("list", _)) => Ok(WalletCommand::List),
        Some(("balance", _)) => Ok(WalletCommand::Balance),
        _ => anyhow::bail!("Invalid wallet command"),
    }
}

fn parse_node_command(matches: &ArgMatches) -> Result<NodeCommand> {
    match matches.subcommand() {
        Some(("start", _)) => Ok(NodeCommand::Start),
        Some(("stop", _)) => Ok(NodeCommand::Stop),
        Some(("status", _)) => Ok(NodeCommand::Status),
        _ => anyhow::bail!("Invalid node command"),
    }
}

fn parse_rpc_command(matches: &ArgMatches) -> Result<RpcCommand> {
    match matches.subcommand() {
        Some(("start", _)) => Ok(RpcCommand::Start),
        Some(("stop", _)) => Ok(RpcCommand::Stop),
        _ => anyhow::bail!("Invalid RPC command"),
    }
}

/// Neo node configuration (matches C# Neo config.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeoConfig {
    pub protocol_configuration: ProtocolConfiguration,
    pub application_configuration: ApplicationConfiguration,
}

/// Protocol configuration (matches C# ProtocolConfiguration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfiguration {
    pub network: u32,
    pub address_version: u8,
    pub milliseconds_per_block: u32,
    pub max_transactions_per_block: u32,
    pub memory_pool_max_transactions: u32,
    pub max_trace_blocks: u32,
    pub initial_gas_distribution: u64,
    pub hardforks: std::collections::HashMap<String, u32>,
    pub committee_members: Vec<String>,
    pub validators_count: u8,
    pub standby_committee: Vec<String>,
    pub standby_validators: Vec<String>,
    pub seed_list: Vec<String>,
}

/// Application configuration (matches C# ApplicationConfiguration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplicationConfiguration {
    pub storage: StorageConfiguration,
    pub p2p: P2PConfiguration,
    pub rpc: RpcConfiguration,
    pub unlock_wallet: UnlockWalletConfiguration,
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfiguration {
    pub engine: String,
    pub path: String,
}

/// P2P network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PConfiguration {
    pub port: u16,
    pub min_desired_connections: u32,
    pub max_connections: u32,
    pub max_known_hashes: u32,
    pub max_connections_per_address: u32,
}

/// RPC server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfiguration {
    pub enabled: bool,
    pub port: u16,
    pub bind_address: String,
    pub ssl_cert: Option<String>,
    pub ssl_cert_password: Option<String>,
    pub trusted_authorities: Vec<String>,
    pub max_concurrent_connections: u32,
    pub disabled_methods: Vec<String>,
}

/// Unlock wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockWalletConfiguration {
    pub path: Option<String>,
    pub password: Option<String>,
    pub is_active: bool,
}

impl Default for NeoConfig {
    fn default() -> Self {
        Self {
            protocol_configuration: ProtocolConfiguration::default(),
            application_configuration: ApplicationConfiguration::default(),
        }
    }
}

impl Default for ProtocolConfiguration {
    fn default() -> Self {
        Self {
            network: 0x334F454E, // "NEO4" in hex (mainnet)
            address_version: 53,
            milliseconds_per_block: MILLISECONDS_PER_BLOCK,
            max_transactions_per_block: MAX_TRANSACTIONS_PER_BLOCK as u32,
            memory_pool_max_transactions: 50000,
            max_trace_blocks: MAX_TRACEABLE_BLOCKS,
            initial_gas_distribution: 52000000_00000000, // 52M GAS
            hardforks: std::collections::HashMap::new(),
            committee_members: vec![
                // Neo mainnet committee members
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                // Additional committee members defined per network configuration
            ],
            validators_count: 7,
            standby_committee: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                // /* implementation */; additional standby committee members
            ],
            standby_validators: vec![
                "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c".to_string(),
                "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093".to_string(),
                // /* implementation */; additional standby validators
            ],
            seed_list: vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
                // Additional reliable seed nodes
                "mainnet1-seed.neocompiler.io:10333".to_string(),
                "mainnet2-seed.neocompiler.io:10333".to_string(),
                "neo-seed.nodes.network:10333".to_string(),
            ],
        }
    }
}

impl Default for ApplicationConfiguration {
    fn default() -> Self {
        Self {
            storage: StorageConfiguration::default(),
            p2p: P2PConfiguration::default(),
            rpc: RpcConfiguration::default(),
            unlock_wallet: UnlockWalletConfiguration::default(),
        }
    }
}

impl Default for StorageConfiguration {
    fn default() -> Self {
        Self {
            engine: "RocksDB".to_string(),
            path: "Data_LevelDB_{0}".to_string(),
        }
    }
}

impl Default for P2PConfiguration {
    fn default() -> Self {
        Self {
            port: 10333,
            min_desired_connections: 10,
            max_connections: 40,
            max_known_hashes: MAX_SCRIPT_SIZE as u32,
            max_connections_per_address: 3,
        }
    }
}

impl Default for RpcConfiguration {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 10332,
            bind_address: "localhost".to_string(),
            ssl_cert: None,
            ssl_cert_password: None,
            trusted_authorities: vec![],
            max_concurrent_connections: 40,
            disabled_methods: vec![],
        }
    }
}

impl Default for UnlockWalletConfiguration {
    fn default() -> Self {
        Self {
            path: None,
            password: None,
            is_active: false,
        }
    }
}

impl NeoConfig {
    /// Load configuration from file
    pub fn load_from_file(path: &std::path::Path) -> Result<Self> {
        if path.exists() {
            let content =
                std::fs::read_to_string(path).context("Failed to read configuration file")?;
            let config: NeoConfig =
                serde_json::from_str(&content).context("Failed to parse configuration file")?;
            Ok(config)
        } else {
            // Create default configuration and save it
            let config = Self::default();
            config.save_to_file(path)?;
            Ok(config)
        }
    }

    /// Save configuration to file
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize configuration")?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create configuration directory")?;
        }

        std::fs::write(path, content).context("Failed to write configuration file")?;

        Ok(())
    }

    /// Apply CLI overrides to configuration
    pub fn apply_cli_overrides(&mut self, args: &CliArgs) {
        // Override network settings
        match args.network {
            NetworkType::Mainnet => {
                self.protocol_configuration.network = 0x334F454E;
            }
            NetworkType::Testnet => {
                self.protocol_configuration.network = 0x3254334E;
            }
        }

        // Override P2P port
        self.application_configuration.p2p.port = args.p2p_port;

        // Override RPC settings
        self.application_configuration.rpc.enabled = args.rpc_enabled;
        self.application_configuration.rpc.port = args.rpc_port;

        // Override wallet settings
        if let Some(ref wallet_path) = args.wallet {
            self.application_configuration.unlock_wallet.path =
                Some(wallet_path.to_string_lossy().to_string());
            self.application_configuration.unlock_wallet.password = args.wallet_password.clone();
            self.application_configuration.unlock_wallet.is_active = true;
        }

        // Override storage path
        self.application_configuration.storage.path =
            format!("Data_LevelDB_{:08X}", self.protocol_configuration.network);
    }
}

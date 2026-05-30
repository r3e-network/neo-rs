//! # Neo Config
//!
//! Configuration management for Neo N3 blockchain node.
//!
//! This crate provides:
//! - Node settings (network, storage, logging)
//! - Protocol parameters (block time, validators, fees)
//! - Network configuration (`MainNet`, `TestNet`, private networks)
//! - Genesis block configuration
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_config::{Settings, NetworkType};
//!
//! // Load from file
//! let settings = Settings::from_file("config.toml")?;
//!
//! // Or use defaults for a network
//! let settings = Settings::default_for_network(NetworkType::MainNet);
//! ```

mod consensus_settings;
mod error;
mod genesis;
pub mod hardfork;
mod logging_settings;
mod network_config;
mod network_type;
mod node_settings;
mod protocol;
mod rpc_settings;
mod settings;
mod storage_settings;
mod telemetry_settings;

pub use consensus_settings::ConsensusSettings;
pub use error::{ConfigError, ConfigResult};
pub use genesis::{GenesisConfig, GenesisValidator};
pub use hardfork::{is_hardfork_enabled, Hardfork, HardforkManager, HardforkParseError};
pub use logging_settings::LoggingSettings;
pub use network_config::NetworkConfig;
pub use network_type::NetworkType;
pub use node_settings::NodeSettings;
pub use protocol::ProtocolSettings;
pub use rpc_settings::RpcSettings;
pub use settings::Settings;
pub use storage_settings::StorageSettings;
pub use telemetry_settings::TelemetrySettings;

/// Current configuration version for migration support
pub const CONFIG_VERSION: u32 = 1;

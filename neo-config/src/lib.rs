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

mod error;
mod genesis;
pub mod hardfork;
mod network_type;
mod protocol;

pub use error::{ConfigError, ConfigResult};
pub use genesis::{GenesisConfig, GenesisValidator};
pub use hardfork::{is_hardfork_enabled, Hardfork, HardforkManager, HardforkParseError};
pub use network_type::NetworkType;
pub use protocol::ProtocolSettings;

/// Current configuration version for migration support
pub const CONFIG_VERSION: u32 = 1;

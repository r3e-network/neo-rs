#![deny(unsafe_code)]
#![warn(missing_docs)]

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
//! use neo_config::{NetworkType, ProtocolSettings};
//!
//! // Use the built-in defaults, or load from a config file / JSON value.
//! let settings = ProtocolSettings::default();
//! let network = NetworkType::MainNet;
//! ```

mod error;
mod genesis;
pub mod hardfork;
mod network_type;
mod protocol;

pub use error::{ConfigError, ConfigResult};
pub use genesis::{GenesisConfig, GenesisValidator};
pub use hardfork::{Hardfork, HardforkManager, HardforkParseError, is_hardfork_enabled};
pub use network_type::NetworkType;
pub use protocol::ProtocolSettings;

/// Current configuration version for migration support
pub const CONFIG_VERSION: u32 = 1;

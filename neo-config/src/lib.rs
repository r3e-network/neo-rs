//! # neo-config
//!
//! Network, protocol, and hardfork configuration records for Neo N3 nodes.
//!
//! ## Boundary
//!
//! This configuration crate owns typed settings and must not open storage,
//! start services, or run protocol workflows.
//!
//! ## Contents
//!
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `network`: operator network status and peer view screen.
//! - `settings`: Protocol settings, hardfork gates, and node configuration
//!   records.

mod errors;
mod network;
mod settings;

pub use errors::{ConfigError, ConfigResult, error};
pub use network::genesis::{GenesisConfig, GenesisValidator};
pub use network::network_type::NetworkType;
pub use network::{genesis, network_type};
pub use settings::{Hardfork, HardforkManager, HardforkParseError, ProtocolSettings, hardfork};

/// Current configuration version for migration support
pub const CONFIG_VERSION: u32 = 1;

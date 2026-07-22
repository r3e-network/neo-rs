//! # neo-config
//!
//! Immutable chain specifications and protocol configuration for Neo N3 nodes.
//!
//! ## Boundary
//!
//! This crate owns immutable chain identity, deterministic genesis inputs,
//! protocol limits, hardfork schedules, and their parsers. Operator runtime
//! policy such as transaction-pool capacity belongs to the consuming service.
//! This crate must not open storage, start services, or run protocol workflows.
//!
//! ## Contents
//!
//! - `network`: chain identity, deterministic genesis data, and network
//!   identifiers.
//! - `settings`: protocol settings and ordered hardfork schedules.

mod network;
mod settings;

pub use network::chain_spec::{ChainIdentity, ChainSpecError, ChainSpecProvider, NeoChainSpec};
pub use network::genesis::{
    GENESIS_NONCE, GENESIS_TIMESTAMP_MS, GenesisConfig, GenesisConfigError, GenesisValidator,
};
pub use network::network_type::{NetworkType, NetworkTypeParseError};
pub use network::{genesis, network_type};
pub use settings::{
    ActiveHardforks, Hardfork, HardforkParseError, HardforkSchedule, HardforkScheduleError,
    ProtocolConfigError, ProtocolSettings, hardfork,
};

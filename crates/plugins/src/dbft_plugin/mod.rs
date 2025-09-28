//! DBFT Plugin - Consensus Plugin for Neo Blockchain
//!
//! This module provides the DBFT (Delegated Byzantine Fault Tolerance) consensus
//! implementation as a plugin, matching the C# Neo.Plugins.DBFTPlugin exactly.

pub mod consensus;
pub mod dbft_plugin;
pub mod dbft_settings;
pub mod messages;
pub mod types;

// Re-export commonly used types
pub use consensus::consensus_service::ConsensusService;
pub use dbft_plugin::DBFTPlugin;
pub use dbft_settings::DbftSettings;
pub use messages::*;
pub use types::*;
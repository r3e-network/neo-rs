//! DBFT Plugin - Consensus Plugin for Neo Blockchain.
//!
//! The full consensus implementation is always compiled; no stubbed mode.

pub mod consensus;
pub mod dbft_settings;
pub mod messages;
pub mod plugin;
pub mod types;

// Re-export commonly used types
pub use consensus::consensus_service::ConsensusService;
pub use dbft_settings::DbftSettings;
pub use messages::*;
pub use plugin::DBFTPlugin;
pub use types::*;

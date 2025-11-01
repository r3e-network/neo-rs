//! DBFT Plugin - Consensus Plugin for Neo Blockchain
//!
//! When the `dbft-full` feature is enabled we expose the full consensus implementation.
//! Otherwise we provide a lightweight stub so the crate still compiles while the port
//! is in progress.

#[cfg(feature = "dbft-full")]
pub mod consensus;
#[cfg(feature = "dbft-full")]
pub mod dbft_plugin;
#[cfg(feature = "dbft-full")]
pub mod dbft_settings;
#[cfg(feature = "dbft-full")]
pub mod messages;
#[cfg(feature = "dbft-full")]
pub mod types;

#[cfg(not(feature = "dbft-full"))]
mod stub;

// Re-export commonly used types
#[cfg(feature = "dbft-full")]
pub use consensus::consensus_service::ConsensusService;
#[cfg(feature = "dbft-full")]
pub use dbft_plugin::DBFTPlugin;
#[cfg(feature = "dbft-full")]
pub use dbft_settings::DbftSettings;
#[cfg(feature = "dbft-full")]
pub use messages::*;
#[cfg(feature = "dbft-full")]
pub use types::*;

#[cfg(not(feature = "dbft-full"))]
pub use stub::{ConsensusService, DBFTPlugin, DbftSettings};

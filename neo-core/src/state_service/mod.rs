//! State Service Module
//!
//! This module provides the state root computation and verification service
//! that matches the C# StateService plugin exactly.

pub(crate) mod commit_handlers;
pub mod keys;
pub mod metrics;
pub mod state_root;
pub mod state_store;
pub mod vote;

/// Extensible payload category for state service messages (matches C# StateService.StatePayloadCategory).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

/// StateService network MessageType prefix values (matches Neo.Plugins.StateService.Network.MessageType).
pub const STATE_SERVICE_MESSAGE_VOTE: u8 = 0;
pub const STATE_SERVICE_MESSAGE_STATE_ROOT: u8 = 1;

pub use keys::Keys;
pub use metrics::StateRootIngestStats;
pub use state_root::StateRoot;
pub use state_store::{StateStore, StateStoreTransaction};
pub use vote::Vote;

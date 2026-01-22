//! State Service Module
//!
//! This module provides the state root computation and verification service
//! that matches the C# StateService plugin exactly.

pub(crate) mod commit_handlers;
pub mod message_type;
pub mod keys;
pub mod metrics;
pub mod state_root;
pub mod state_store;
pub mod vote;
#[cfg(feature = "runtime")]
pub mod verification;

/// Extensible payload category for state service messages (matches C# StateService.StatePayloadCategory).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::StateRootIngestStats;
pub use state_root::StateRoot;
pub use state_store::{StateStore, StateStoreTransaction};
pub use vote::Vote;

/// Event published when a validated state root is persisted.
#[derive(Debug, Clone)]
pub struct ValidatedRootPersisted {
    pub index: u32,
}

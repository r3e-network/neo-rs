//! State Service Module
//!
//! This module provides the state root computation and verification service
//! that matches the C# StateService plugin exactly.

pub mod keys;
pub mod metrics;
pub mod state_root;
pub mod state_store;
pub(crate) mod commit_handlers;

/// Extensible payload category for state service messages (matches C# StateService.StatePayloadCategory).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use metrics::StateRootIngestStats;
pub use state_root::StateRoot;
pub use state_store::{StateStore, StateStoreTransaction};

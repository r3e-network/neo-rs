//! State-service integration owned by the node runtime (task W3-C2).
//!
//! The state root computation/verification service lives in the
//! `neo-state-service` crate (re-exported from `neo_core::state_service`).
//! The commit pipeline that wires state root calculation into block
//! persistence — [`commit_handlers`] — is owned by this crate because it is
//! driven by the blockchain actor and the `NeoSystemContext` handler
//! registry, both of which live here.

mod state_root;
pub mod commit_handlers;

/// Test-only helpers for building native-contract storage fixtures.
///
/// Only consumer is `crate::blockchain::tests`.
#[cfg(test)]
pub(crate) mod test_support;

// Re-export state root types
pub use state_root::{STATE_SERVICE_CATEGORY, StateRoot};
pub use commit_handlers::StateServiceCommitHandlers;

/// Event emitted when a validated state root is persisted.
#[derive(Debug, Clone)]
pub struct ValidatedRootPersisted {
    pub index: u32,
}

/// State service settings (placeholder for removed state service).
#[derive(Debug, Clone)]
pub struct StateServiceSettings {
    pub enabled: bool,
}

//! State-service integration owned by the node runtime (task W3-C2).
//!
//! The state root computation/verification service lives in the
//! `neo-state-service` crate (re-exported from `neo_core::state_service`).
//! The commit pipeline that wires state root calculation into block
//! persistence — [`commit_handlers`] — is owned by this crate because it is
//! driven by the blockchain actor and the `NeoSystemContext` handler
//! registry, both of which live here.

// pub mod commit_handlers;  // Removed during consolidation (state-service removed)

/// Test-only helpers for building native-contract storage fixtures.
///
/// Only consumer is `crate::blockchain::tests`.
#[cfg(test)]
pub(crate) mod test_support;

// pub use commit_handlers::StateServiceCommitHandlers;  // Removed during consolidation

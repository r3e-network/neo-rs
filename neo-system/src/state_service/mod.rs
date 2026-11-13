//! State-service integration owned by the node runtime (task W3-C2).
//!
//! The state root computation/verification service lives in the
//! `neo-state-service` crate (re-exported from `neo_core::state_service`).
//! The commit pipeline that wires state root calculation into block
//! persistence — [`commit_handlers`] — is owned by this crate because it is
//! driven by the blockchain actor and the `NeoSystemContext` handler
//! registry, both of which live here.

pub mod commit_handlers;
mod state_root;
mod store;

/// Test-only helpers for building native-contract storage fixtures.
///
/// Only consumer is `crate::blockchain::tests`.
#[cfg(test)]
pub(crate) mod test_support;

// Re-export state root types
pub use commit_handlers::{
    StateRootCommitProfile, StateRootNodeCacheStats, StateRootProfileSink,
    StateServiceCommitHandlers,
};
pub use state_root::{STATE_SERVICE_CATEGORY, StateRoot};
pub(crate) use store::StateRootStore;

/// Event emitted when a validated state root is persisted.
#[derive(Debug, Clone)]
pub struct ValidatedRootPersisted {
    pub index: u32,
}

/// State service settings for state root computation and verification.
///
/// Configures the state root tracking service that:
/// - Computes and validates MPT state roots
/// - Verifies state root signatures from committee nodes
/// - Provides state proof generation and verification
///
/// # Implementation Reference
///
/// Mirrors C# Neo `StateService` plugin configuration:
/// - `Neo.Plugins.StateService.Settings`
/// - Used by `Neo.Plugins.StateService.StatePlugin`
#[derive(Debug, Clone)]
pub struct StateServiceSettings {
    /// Whether to maintain full state trie (vs. pruned state).
    pub full_state: bool,

    /// Database path for state store (supports {0} for network substitution).
    pub path: String,

    /// Network magic value for validation.
    pub network: u32,

    /// Enable automatic state root verification when wallet changes.
    pub auto_verify: bool,

    /// Maximum items returned by state service queries.
    pub max_find_result_items: usize,

    /// Policy for handling unhandled exceptions.
    pub exception_policy: UnhandledExceptionPolicy,
}

/// Policy for handling unhandled exceptions in plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnhandledExceptionPolicy {
    /// Ignore exceptions and continue.
    Ignore,
    /// Stop the plugin on exception.
    StopPlugin,
    /// Stop the entire node on exception.
    StopNode,
}

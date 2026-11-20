//! State-root computation, persistence, and verification service.
//!
//! Phase 1 S3: extracted from `neo-system::state_service` (TypeId identity
//! preserved: `neo_system::state_service` re-exports this crate). The commit
//! pipeline — [`commit_handlers`] — computes MPT state roots during block
//! persistence through the [`StateCommitContext`] port; the runtime glue that
//! adapts `NeoSystemContext` to the port and implements the shared blockchain
//! handler traits lives in neo-system (`state_service_glue`).
//!
//! The optional `state-verifier` feature adds [`state_verification`]: signed
//! state-root verification against the unified signing service.

pub mod commit_handlers;
pub mod context;
pub mod services;
mod state_root;
mod store;

#[cfg(feature = "state-verifier")]
pub mod state_verification;

/// Test helpers for building native-contract storage fixtures.
///
/// Phase 1 S3: always compiled and public so cross-crate consumers
/// (`neo-system::blockchain::tests`, state-verification tests) can build the
/// same role-designation records.
pub mod test_support;

// Re-export state root types
pub use commit_handlers::{
    StateRootCommitProfile, StateRootNodeCacheStats, StateRootProfileSink,
    StateServiceCommitHandlers,
};
pub use context::StateCommitContext;
pub use services::StateStoreService;
pub use state_root::{STATE_SERVICE_CATEGORY, StateRoot};
pub use store::StateRootStore;

// State verification surface (feature-gated, mirrors the historical
// neo-system::state_verification module).
#[cfg(feature = "state-verifier")]
pub use state_verification::{
    StateRootSource, StateServiceVerification, StateVerificationError, StoreStateRootSource,
    verify_merkle_proof, verify_state_root,
};

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
///
/// Owned by `neo-primitives` so every plugin-style service shares one policy
/// vocabulary; it is re-exported here for this crate's settings surface.
pub use neo_primitives::UnhandledExceptionPolicy;

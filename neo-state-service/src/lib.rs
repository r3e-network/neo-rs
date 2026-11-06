//! `neo-state-service` — canonical state root computation / verification service.
//!
//! This crate is the destination of the state-service lane of the `neo-core`
//! decomposition (task-107 wave, completed by task-143). It is the single
//! authority for the state root Merkle-Patricia-trie storage ([`state_store`]),
//! the wire-level state root payload ([`state_root`]) and the block-persistence
//! integration point that wires local state root calculation into the canonical
//! commit gate ([`participant`]).
//!
//! # Commit participation
//!
//! The legacy core split state root calculation across two event handlers
//! (`CommittingHandler` + `CommittedHandler`). The canonical persistence
//! pipeline unifies both roles into a single `CommitParticipant`:
//!
//! | participant phase | legacy handler | `StateStore` operation |
//! | --- | --- | --- |
//! | `precommit` (step 12) | `blockchain_committing_handler` | `update_local_state_root_from_cache` |
//! | `discard` (steps 12/13/14 rollback, reverse order) | *(none — was implicit)* | `discard_staged_local_state_root` |
//! | `committed` (step 15a, infallible) | `blockchain_committed_handler` | `update_local_state_root` |
//!
//! [`StateServiceCommitParticipant`](participant::StateServiceCommitParticipant)
//! implements that mapping. Every behavioural guarantee of the legacy handlers is
//! preserved inside the participant: the fail-closed reference-root comparison,
//! the 2000-block trie cache recycle, the `disabled` flag with the five
//! `UnhandledExceptionPolicy` dispositions, `catch_unwind` protection of the
//! staging path, the exact panic/error messages and the fast-sync opt-in.
//!
//! # Module map
//!
//! | module | role | gate |
//! | --- | --- | --- |
//! | [`keys`] | state-service storage key prefixes | always |
//! | [`message_type`] | state-service wire message discriminator | always |
//! | [`metrics`] | state root ingestion / async-MPT counters | always |
//! | [`vote`] | validator vote payload | always |
//! | [`state_root`] | `StateRoot` payload, hashing, witness verification | always |
//! | [`root_cache`] | LRU cache of recent state roots | always |
//! | [`state_store`] | MPT-backed local/validated root store (+ subtree) | always |
//! | [`tiered_storage`] | hot/warm/cold trie node tiering | always |
//! | [`read_only_store`] | read-only overlay over a store snapshot | always |
//! | [`async_state_root`] | worker-pool state root computation | always |
//! | [`participant`] | canonical `CommitParticipant` integration | always |
//! | [`conflict_detector`] | RwSet conflict detection | `runtime` |
//! | [`optimistic_executor`] | optimistic parallel execution pool | `runtime` |
//! | [`parallel_execution_integration`] | block-level parallel execution glue | `runtime` |
//! | [`prefetch_pipeline`] | block prefetch / pipeline stages | `runtime` |
//! | [`prefetch_background`] | background prefetch bookkeeping | `prefetch` |
//! | [`prefetch_worker`] | `BlockPrefetchService` worker | `prefetch` |
//!
//! # Dependency direction
//!
//! Foundation-layer and protocol-execution-layer dependencies only:
//! `neo-storage`, `neo-io`, `neo-crypto`, `neo-primitives`, `neo-protocol`,
//! `neo-vm`, `neo-smart-contract`, `neo-native-contracts`. This crate has **no**
//! dependency on `neo-core` or `neo-system`.
//!
//! The single `impl CommitParticipant` block plus the `impl VerifiableExt`
//! block live behind the `ledger-participant` cfg gate because both traits are
//! owned by `neo-ledger` (an L1 crate — a legal downward edge for an L2 service,
//! but disabled by default so the crate builds ledger-free).

pub mod async_state_root;
pub mod keys;
pub mod message_type;
/// Metrics counters for state root ingestion from the network.
pub mod metrics;
pub mod participant;
pub mod read_only_store;
pub mod root_cache;
pub mod services;
pub mod state_root;
pub mod state_store;
pub mod tiered_storage;
pub mod vote;

// Optimistic/parallel execution lane. Gated exactly as it was inside
// `neo-core::state_service` so the default lib still builds without the
// pipeline subsystem.
#[cfg(feature = "runtime")]
pub mod conflict_detector;
#[cfg(feature = "runtime")]
pub mod optimistic_executor;
#[cfg(feature = "runtime")]
pub mod parallel_execution_integration;
#[cfg(feature = "runtime")]
pub mod prefetch_pipeline;

// WIP: the prefetch subsystem (background worker, BlockPrefetchService) is gated
// behind the `prefetch` feature (not in default), mirroring `neo-core`.
#[cfg(feature = "prefetch")]
pub mod prefetch_background;
#[cfg(feature = "prefetch")]
pub mod prefetch_worker;

/// Extensible payload category for state service messages (matches C# `StateService.StatePayloadCategory`).
pub const STATE_SERVICE_CATEGORY: &str = "StateService";

pub use keys::Keys;
pub use message_type::MessageType;
pub use metrics::StateRootIngestStats;
pub use participant::StateServiceCommitParticipant;
pub use root_cache::{
    DEFAULT_ROOT_CACHE_CAPACITY, StateRootCache, StateRootCacheEntry, StateRootCacheStats,
};
pub use services::StateStoreService;
pub use state_root::StateRoot;
pub use state_store::{StateRootDivergence, StateStore, StateStoreTransaction};
pub use vote::Vote;

/// Event published when a validated state root is persisted.
#[derive(Debug, Clone)]
pub struct ValidatedRootPersisted {
    /// Block index (height) of the validated state root that was persisted.
    pub index: u32,
}

/// Test-only helpers shared by the migrated `state_root` / `state_store` test
/// suites.
///
/// `RoleManagement::serialize_public_keys` is `pub(crate)` inside
/// `neo-native-contracts`, so the designation payload used by the migrated tests
/// is reproduced here through the same public `neo-vm` codec it delegates to
/// (`StackValue::Array` of compressed points + `BinarySerializer`). The bytes are
/// identical; only the call path differs.
#[cfg(test)]
pub(crate) mod test_support {
    use neo_crypto::ECPoint;
    use neo_vm::{BinarySerializer, ExecutionEngineLimits, StackValue};

    /// Serializes designated public keys exactly as
    /// `RoleManagement::serialize_public_keys` does.
    pub(crate) fn serialize_designated_public_keys(
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, String> {
        let value = StackValue::Array(
            public_keys
                .iter()
                .map(|pubkey| StackValue::ByteString(pubkey.as_bytes().to_vec()))
                .collect(),
        );
        BinarySerializer::serialize_stack_value(&value, &ExecutionEngineLimits::default())
            .map_err(|e| format!("Failed to serialize public keys: {e}"))
    }
}

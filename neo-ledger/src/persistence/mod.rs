//! Canonical, pure block-persistence operation for the ledger.
//!
//! [`persist_block`] is a free function with no `self`, no actor system, no networking
//! and no concrete engine: every side-effecting collaborator is supplied as an inverted
//! port ([`SnapshotStore`], [`EngineFactory`], [`RuntimeCacheSink`], [`EvidenceSink`])
//! plus an ordered slice of [`CommitParticipant`]s. This keeps `neo-ledger` free of any
//! `ledger → core / system / p2p / mempool / native` reverse edge; the only outward
//! dependency is a *type-only* edge to `neo-smart-contract` (task-102 L.1 decision A).
//!
//! # The 15-step canonical order (immutable)
//!
//! The step order below is a hard contract (task-102 Part A). It reproduces the legacy
//! `NeoSystem::persist_block_internal` pipeline exactly; reordering any step changes
//! consensus-critical behaviour.
//!
//! 1.  Resolve [`PersistOptions`] and gate detailed-execution emission
//!     (`detailed = !fast_sync || any participant opts into fast-sync`).
//! 2.  Project the canonical block identity (hash, height) and allocate the *bounded*
//!     fault-evidence buffer sized from [`EvidenceSink::available_capacity`].
//! 3.  Open the store write transaction over the current snapshot ([`SnapshotStore`]).
//! 4.  Derive the base overlay snapshot that accumulates the block's committed state.
//! 5.  Pre-compute transaction hashes (fail-closed before any engine work).
//! 6.  `OnPersist`: open the native engine, run `native_on_persist`,
//!     `record_native_persist_success`.
//! 7.  Emit the `OnPersist` [`ExecutionReceipt`] when detailed.
//! 8.  Seed native-only contract states and the shared native cache handle for children.
//! 9.  Take the [`LedgerTransactionStates`] token from the `OnPersist` engine.
//! 10. Per-transaction application loop over one shared child overlay:
//!     set/take state, `load_script`, `execute_allow_fault`; emit a detailed receipt or
//!     **record the VM state unconditionally**; then dispose by VM state:
//!     * **10a `HALT`** — merge the child overlay into the base snapshot.
//!     * **10b `FAULT`** — reset the child overlay and stage a bounded
//!       [`FaultObservation`] (pure category, no observation schema).
//!     * **10c other** — abort with [`LedgerError::UnexpectedVmState`].
//! 11. `PostPersist`: open the seeded native engine, set state, run `native_post_persist`,
//!     `record_native_persist_success`, emit the receipt when detailed.
//! 12. Commit participants **stage/precommit in registration order**; the first
//!     `Err` triggers **reverse-order discard** of the staging list — which
//!     includes the participant that just failed — and returns
//!     [`LedgerError::ParticipantCommit`] (Display embeds the participant message).
//! 13. **Store commit** ([`StoreTransactionPort::commit`]); on failure reverse-discard
//!     and return [`LedgerError::StoreCommit`] (Display carries the store commit prefix).
//! 14. **Runtime cache / tip update**: `record_block` (online) or `record_tip` (offline
//!     import). This runs *after* the durable commit, so a failure returns
//!     [`LedgerError::PostCommitCacheFailure`] — never a rollback, and never a
//!     "nothing committed" error.
//! 15. **Infallible post-commit** (never reorders, never fails):
//!     * **15a** committed participants run in registration order for *all*
//!       participants regardless of fast-sync gating.
//!     * **15b** bounded fault-evidence export through [`EvidenceSink`]; a missing,
//!       full or closed sink only increments the dropped count.
//!     * **15c** plugin `BlockReceived` broadcast when detailed.
//!
//! Returns a [`PersistReceipt`] carrying the ordered receipts and evidence accounting.

pub mod engine_port;
pub mod error;
pub mod evidence;
pub mod participant;
mod persist;
pub mod profile;
pub mod receipt;
pub mod transaction_loop;

#[cfg(test)]
mod block_processing_tests;
#[cfg(test)]
mod tests;

pub use engine_port::{
    ApplicationEnginePort, EngineFactory, NativeCacheHandle, NativeEnginePort,
    SeededNativeContracts, SpeculativeExecution,
};
pub use error::{LedgerError, LedgerResult};
pub use evidence::{
    EvidenceSink, FaultCategory, FaultObservation, MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK,
};
pub use participant::{CommitContext, CommitParticipant, StageContext};
pub use persist::{persist_block, persist_block_profiled, persist_block_with_loop_strategy};
pub use profile::{PersistenceProfiler, PersistenceStage};
pub use receipt::{ExecutionReceipt, PersistReceipt};
pub use transaction_loop::{
    TransactionLoopContext, TransactionLoopOutcome, TransactionLoopStrategy, execute_serially,
};

use profile::StageTimer;

use evidence::FaultObservationBuffer;
use neo_protocol::execution::Block;
use neo_smart_contract::application_engine::LedgerTransactionStates;
use neo_storage::persistence::PersistenceDataCache;
use std::sync::Arc;

/// Options selecting the persistence mode for one [`persist_block`] invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistOptions {
    /// Fast-sync mode suppresses detailed receipts unless a participant opts in.
    pub fast_sync: bool,
    /// When `true`, the runtime caches are updated with the block (online sync); when
    /// `false`, only the tip height advances (offline import).
    pub update_runtime_cache: bool,
}

impl PersistOptions {
    /// Online sync: detailed receipts, runtime cache updated.
    pub const fn online() -> Self {
        Self {
            fast_sync: false,
            update_runtime_cache: true,
        }
    }

    /// Offline import: detailed receipts, tip-only advance (no runtime block cache).
    pub const fn offline_import() -> Self {
        Self {
            fast_sync: false,
            update_runtime_cache: false,
        }
    }

    /// Fast-sync import: gated receipts, runtime cache updated.
    pub const fn fast_sync() -> Self {
        Self {
            fast_sync: true,
            update_runtime_cache: true,
        }
    }
}

impl Default for PersistOptions {
    fn default() -> Self {
        Self::online()
    }
}

/// Canonical write transaction over the store snapshot for one block persist.
///
/// Owned by the `neo-system`/storage adapter. The ledger only threads the base overlay
/// and issues the single canonical commit.
pub trait StoreTransactionPort: Send {
    /// The base overlay snapshot accumulating the block's committed state (step 4).
    ///
    /// Returns a shared handle; the pipeline derives per-transaction child overlays via
    /// [`PersistenceDataCache::clone_cache`] and merges them back with [`PersistenceDataCache::merge_from_child`].
    fn base_snapshot(&self) -> Arc<PersistenceDataCache>;

    /// Commits the staged canonical block state to the underlying store (step 13).
    ///
    /// The `String` is the store-owned failure message; the pipeline lifts it into
    /// [`LedgerError::StoreCommit`] behind the canonical store commit prefix.
    fn commit(self: Box<Self>) -> Result<(), String>;
}

/// Supplies the canonical store write transaction (steps 3/13).
pub trait SnapshotStore: Send + Sync {
    /// Opens the write transaction over the current store snapshot.
    fn open_transaction(
        &self,
        options: &PersistOptions,
    ) -> Result<Box<dyn StoreTransactionPort>, LedgerError>;
}

/// Runtime-facing sink for cache/tip updates and the plugin broadcast (steps 14/15c).
pub trait RuntimeCacheSink: Send + Sync {
    /// Records the block into the online runtime caches (step 14, online).
    ///
    /// The `String` is the sink-owned failure message; the pipeline lifts it into
    /// [`LedgerError::PostCommitCacheFailure`] because the store commit has
    /// already succeeded by the time this runs.
    fn record_block(&self, block: &Block) -> Result<(), String>;

    /// Advances the tip height for offline imports (step 14, offline). Infallible.
    fn record_tip(&self, index: u32);

    /// Broadcasts the plugin `BlockReceived` event (step 15c). Infallible.
    fn broadcast_block_received(&self, block_hash: String, block_height: u32);
}

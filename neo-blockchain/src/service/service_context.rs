//! System context trait used by [`crate::service::BlockchainService`].
//!
//! The trait is the seam between the blockchain service and the
//! rest of the node: it provides the service with access to the
//! protocol settings, the current chain height, and the storage /
//! mempool / network backends it needs to validate and persist
//! blocks.
//!
//! Concrete implementations live in node and test contexts. The production
//! daemon context in `neo-node` exposes the canonical store snapshot and commit
//! hook, while tests can provide smaller in-memory contexts.
//!
//! The trait surface is intentionally narrow: it covers the operations the
//! service actually uses and keeps node/application wiring outside this crate.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};

/// Extra metadata for the current block persistence call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockPersistContext {
    /// True for trusted local bootstrap/import paths such as chain.acc and
    /// built-in fast-sync packages.
    pub bulk_sync: bool,
}

impl BlockPersistContext {
    /// Normal live-network/consensus persistence.
    #[must_use]
    pub const fn live() -> Self {
        Self { bulk_sync: false }
    }

    /// Trusted local bootstrap/import persistence.
    #[must_use]
    pub const fn bulk_sync() -> Self {
        Self { bulk_sync: true }
    }
}

/// System access required by [`crate::service::BlockchainService`].
///
/// Implementations are expected to be cheap to clone and `Send + Sync`. The
/// blockchain service is the *only* owner of the canonical tip, so concurrent
/// callers go through the service's command channel rather than through this
/// trait directly.
pub trait SystemContext: Send + Sync + std::fmt::Debug {
    /// Native-contract provider captured by the composition root.
    type NativeProvider: NativeContractProvider + 'static;

    /// Concrete cache backing captured by the composition root.
    type CacheBacking: neo_storage::CacheRead;

    /// Returns the effective protocol settings.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the current canonical chain height.
    fn current_height(&self) -> u32;

    /// Returns a data-cache snapshot over the canonical store for block
    /// persistence, or `None` when the implementation exposes no store
    /// (e.g. lightweight test contexts). When this returns a snapshot,
    /// [`crate::service::BlockchainService`] runs the native-contract
    /// persistence pipeline
    /// ([`crate::native_persist::persist_block_natives_with_resources`]) against
    /// it for every persisted block; committing the snapshot to the backing
    /// store remains the implementation's responsibility.
    fn store_snapshot(&self) -> Option<Arc<neo_storage::DataCache<Self::CacheBacking>>> {
        None
    }

    /// Returns the native-contract provider captured by the composition root,
    /// when this context owns one.
    ///
    /// Blockchain handlers use this for witness verification and persistence
    /// paths that execute native-contract lookups. Lightweight tests and
    /// store-less contexts can leave it unset only when they do not expose a
    /// store-backed persistence path.
    fn native_contract_provider(&self) -> Option<Arc<Self::NativeProvider>> {
        None
    }

    /// Called after a block's native persistence pipeline has produced its
    /// `ApplicationExecuted` records but before the canonical store is
    /// committed. This mirrors the C# `ICommittingHandler` plugin hook and lets
    /// stateful plugins consume the same snapshot that will be committed to the
    /// canonical store. Returning `false` aborts block persistence; handlers
    /// should reserve that for consensus-critical failures (for example
    /// StateService local MPT updates).
    fn block_committing(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache<Self::CacheBacking>,
        _application_executed_list: &[ApplicationExecuted],
    ) -> bool {
        true
    }

    /// Called after native persistence has produced writes for a block and
    /// before the canonical store commit, with caller metadata for catch-up
    /// decisions. Implementations that do not care about the metadata can keep
    /// overriding [`SystemContext::block_committing`].
    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &neo_storage::DataCache<Self::CacheBacking>,
        application_executed_list: &[ApplicationExecuted],
        _context: BlockPersistContext,
    ) -> bool {
        self.block_committing(block, snapshot, application_executed_list)
    }

    /// Flushes the writes accumulated in [`SystemContext::store_snapshot`] (after
    /// a block's native-persist pipeline) through to the durable backing store.
    /// The blockchain service calls this once per successfully persisted block
    /// (mirroring C# `snapshot.Commit()` at the end of `Blockchain.Persist`).
    /// The default is a successful no-op for store-less test contexts.
    ///
    /// A commit error must reach the import path, and implementations must
    /// discard the failed root overlay before returning it. Logging and
    /// continuing would let in-memory tip state and post-commit observers
    /// advance past the last durable block.
    fn commit_to_store(&self) -> Result<(), String> {
        Ok(())
    }

    /// Discards the canonical overlay after a pre-commit fence or durable
    /// backend commit fails.
    ///
    /// Store-backed implementations must restore reads to the last durable
    /// snapshot. The default remains a no-op for store-less test contexts.
    fn abort_store_commit(&self) {}

    /// Returns whether a fatal persistence condition requires the active
    /// command to abort and the canonical writer loop to stop before
    /// dispatching another command.
    fn should_stop_blockchain_service(&self) -> bool {
        false
    }

    /// Flushes any deferred commit-handler work that was queued during a trusted
    /// bulk-sync batch before the batch is durably committed.
    ///
    /// The default is a no-op. Daemon contexts use this to force the
    /// StateService async MPT worker to surface failures at the batch boundary
    /// instead of deferring them until shutdown.
    fn flush_bulk_sync_commit_handlers(&self) -> Result<(), String> {
        Ok(())
    }

    /// Returns whether a trusted bulk-sync import may skip per-block committing
    /// hooks for a state-equivalent empty-block fast-forward run.
    ///
    /// Implementations must return `false` when any active component needs the
    /// exact per-block `Committing`/`Committed`/application-executed stream
    /// during bulk sync (for example StateService validation, indexers, or
    /// plugin-style observers). The default is conservative.
    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }

    /// Returns whether a trusted bulk-sync import may replace the native VM
    /// OnPersist/PostPersist engines for an empty block with the
    /// state-equivalent empty-block writer while still invoking
    /// [`SystemContext::block_committing_with_context`] and
    /// [`SystemContext::block_committed_with_context`] once per block.
    ///
    /// This is narrower than [`SystemContext::allows_empty_block_fast_forward`]:
    /// it keeps the per-block observer stream and is intended for StateService
    /// catch-up, where each local state root must still be produced but the
    /// empty native-contract side effects can be written directly. Return
    /// `false` when any observer needs full replay artifacts or native-engine
    /// notifications.
    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        false
    }

    /// Called after the block's writes have been committed to the canonical
    /// store. This mirrors the C# `ICommittedHandler` plugin hook.
    fn block_committed(&self, _block: &Block) {}

    /// Called after the block's writes have been committed to the canonical
    /// store, with caller metadata for catch-up decisions. Implementations that
    /// do not care about the metadata can keep overriding
    /// [`SystemContext::block_committed`].
    fn block_committed_with_context(&self, block: &Block, _context: BlockPersistContext) {
        self.block_committed(block);
    }
}

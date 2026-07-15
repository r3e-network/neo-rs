//! Core blockchain system context and application commit-hook boundary.
//!
//! `NodeSystemContext` owns provider-neutral node mechanics: protocol settings,
//! the canonical store snapshot, durable commits, and the native-contract
//! provider. Application-specific StateService and indexing policy is supplied
//! through the statically dispatched `BlockCommitHooks` collaborator.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neo_blockchain::{
    BlockPersistContext, ChainTipProvider, FinalizedBlock, HotColdLedgerProviderFactory,
    LedgerProvider, LedgerProviderFactory, OptionalStaticLedgerProvider, SyncBatchCommitPolicy,
    SystemContext,
};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::StorageResult;
use neo_storage::persistence::{
    RawOverlaySource, StoreCache, StoreCacheBacking, StoreDataCache, TransactionalStore,
};
use parking_lot::Mutex;

/// Canonical durability capability exposed to application commit policy.
///
/// The composition root retains ownership of its concrete [`StoreCache`]. An
/// application hook can request an ordinary commit or supply a stronger atomic
/// backend transaction without depending on cache mechanics or taking storage
/// ownership back from `neo-system`.
pub trait CanonicalCommit<S>
where
    S: TransactionalStore,
{
    /// Ordered canonical overlay type borrowed for an external transaction.
    type Overlay<'a>: RawOverlaySource + ?Sized
    where
        Self: 'a;

    /// Publishes the canonical overlay through the store's normal transaction.
    fn commit_durable(&mut self) -> Result<(), String>;

    /// Publishes the canonical overlay through a stronger external transaction.
    fn commit_durable_with<F>(&mut self, commit: F) -> Result<(), String>
    where
        F: FnOnce(&S, &mut Self::Overlay<'_>) -> StorageResult<()>;

    /// Drops canonical mutations that must not cross a failed durability fence.
    fn discard_pending(&mut self);
}

impl<S> CanonicalCommit<S> for StoreCache<S>
where
    S: TransactionalStore + 'static,
{
    type Overlay<'a>
        = &'a neo_storage::DataCache<StoreCacheBacking<S>>
    where
        Self: 'a;

    fn commit_durable(&mut self) -> Result<(), String> {
        self.try_commit_durable().map_err(|error| error.to_string())
    }

    fn commit_durable_with<F>(&mut self, commit: F) -> Result<(), String>
    where
        F: FnOnce(&S, &mut Self::Overlay<'_>) -> StorageResult<()>,
    {
        self.try_commit_durable_with(commit)
            .map_err(|error| error.to_string())
    }

    fn discard_pending(&mut self) {
        self.discard_pending_changes();
    }
}

/// Application-owned behavior around a canonical block commit.
///
/// `S` is the concrete transactional store used by the composition. Snapshots
/// retain its `StoreCacheBacking<S>` type, while the durability hook can select
/// an ordinary or coordinated store commit without virtual dispatch.
pub trait BlockCommitHooks<S>: Send + Sync + fmt::Debug
where
    S: TransactionalStore,
{
    /// Whether this application composition needs copied execution artifacts.
    ///
    /// This is a conservative capability query. Implementations must not base
    /// it on mutable peer-tip or projection-checkpoint state that can change
    /// before [`BlockCommitHooks::block_committing`] runs. Return `true` when a
    /// configured observer could consume the records in this persistence
    /// context; the later hook may still decide not to consume them.
    ///
    /// Returning `false` removes transaction/result/notification snapshot work
    /// only; it does not skip execution, Ledger VM-state recording, StateService,
    /// static archival, durability hooks, or finalized block publication.
    fn requires_replay_artifacts(&self, _block: &Block, _context: BlockPersistContext) -> bool {
        true
    }

    /// Run pre-commit observers and return whether persistence may continue.
    fn block_committing(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache<StoreCacheBacking<S>>,
        _application_executed: &[ApplicationExecuted],
        _live_tip: u64,
        _context: BlockPersistContext,
    ) -> bool {
        true
    }

    /// Deliver one canonical outcome after the Ledger durability fence.
    fn block_finalized(
        &self,
        _finalized: FinalizedBlock<StoreCacheBacking<S>>,
        _live_tip: u64,
    ) -> impl Future<Output = Result<(), String>> + Send {
        async { Ok(()) }
    }

    /// Whether a verified peer-sync range may share one durable commit.
    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
        _live_tip: u64,
    ) -> SyncBatchCommitPolicy {
        SyncBatchCommitPolicy::PerBlock
    }

    /// Flush deferred work at an import-batch boundary.
    fn flush_deferred(&self) -> Result<(), String> {
        Ok(())
    }

    /// Projected StateService change budget for intermediate deferred commits.
    ///
    /// When set, the blockchain deferred-import loop intermediate-commits once
    /// pending projected MPT changes reach this budget so coordinated MDBX
    /// transactions stay work-bounded. Default disables intermediate flushes.
    fn deferred_import_work_budget(&self) -> Option<usize> {
        None
    }

    /// Pending projected StateService changes awaiting the next deferred commit.
    fn pending_deferred_import_work(&self) -> usize {
        0
    }

    /// Fences pre-commit observer stores before canonical Ledger durability.
    fn fence_precommit_durability(&self) -> Result<(), String> {
        Ok(())
    }

    /// Publishes the canonical cache through the selected durability strategy.
    ///
    /// The default fences independent pre-commit stores and then uses the
    /// canonical store transaction. Application composition may override this
    /// method to include a prepared service namespace in the same transaction.
    fn commit_canonical<C>(&self, canonical: &mut C) -> Result<(), String>
    where
        C: CanonicalCommit<S>,
    {
        if let Err(error) = self.fence_precommit_durability() {
            canonical.discard_pending();
            return Err(format!("pre-commit durability fence failed: {error}"));
        }
        canonical.commit_durable()
    }

    /// Notify application recovery policy after the canonical durability fence.
    fn canonical_commit_succeeded(&self) {}

    /// Notify application recovery policy when canonical publication cannot
    /// safely complete after pre-commit observers may have persisted state.
    fn canonical_commit_failed(&self, _reason: &str) {}

    /// Notify recovery policy when finalized delivery fails after Ledger commit.
    fn finalized_delivery_failed(&self, _reason: &str) {}

    /// Whether application recovery policy has made the canonical writer fatal.
    fn should_stop_blockchain_service(&self) -> bool {
        false
    }

    /// Whether trusted replay may skip per-block commit hooks entirely.
    fn allows_empty_block_fast_forward(&self) -> bool {
        false
    }

    /// Whether empty native persistence may be replaced while retaining hooks.
    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        false
    }
}

/// Commit-hook implementation for compositions with no application observers.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopBlockCommitHooks;

impl<S> BlockCommitHooks<S> for NoopBlockCommitHooks
where
    S: TransactionalStore,
{
    fn requires_replay_artifacts(&self, _block: &Block, _context: BlockPersistContext) -> bool {
        false
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
        _live_tip: u64,
    ) -> SyncBatchCommitPolicy {
        SyncBatchCommitPolicy::DeferredLive
    }
}

/// Provider-neutral context consumed by `neo-blockchain`.
pub struct NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<StoreDataCache<S>>,
    store_cache: Mutex<StoreCache<S>>,
    native_contract_provider: Arc<P>,
    ledger_provider_factory: HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>,
    hooks: Arc<H>,
    fatal_persistence_error: AtomicBool,
}

impl<P, S, H> NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    /// Compose a blockchain context over one canonical store cache.
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<StoreDataCache<S>>,
        store_cache: StoreCache<S>,
        native_contract_provider: Arc<P>,
        hooks: Arc<H>,
    ) -> Self {
        Self::new_with_ledger_provider(
            settings,
            snapshot,
            store_cache,
            native_contract_provider,
            OptionalStaticLedgerProvider::default(),
            hooks,
        )
    }

    /// Compose a blockchain context with an application-selected cold Ledger
    /// fallback.
    pub fn new_with_ledger_provider(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<StoreDataCache<S>>,
        store_cache: StoreCache<S>,
        native_contract_provider: Arc<P>,
        cold_ledger_provider: OptionalStaticLedgerProvider,
        hooks: Arc<H>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            store_cache: Mutex::new(store_cache),
            native_contract_provider,
            ledger_provider_factory: HotColdLedgerProviderFactory::new(cold_ledger_provider),
            hooks,
            fatal_persistence_error: AtomicBool::new(false),
        }
    }

    fn mark_fatal_persistence_error(&self, reason: &str) {
        self.fatal_persistence_error.store(true, Ordering::Release);
        self.hooks.canonical_commit_failed(reason);
    }

    fn mark_fatal_finalized_delivery_error(&self, reason: &str) {
        self.fatal_persistence_error.store(true, Ordering::Release);
        self.hooks.finalized_delivery_failed(reason);
    }
}

impl<P, S, H> fmt::Debug for NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NodeSystemContext")
            .field("network", &self.settings.network)
            .field("hooks", &self.hooks)
            .finish_non_exhaustive()
    }
}

impl<P, S, H> SystemContext for NodeSystemContext<P, S, H>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
    H: BlockCommitHooks<S> + 'static,
{
    type NativeProvider = P;
    type CacheBacking = StoreCacheBacking<S>;

    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        self.ledger_provider_factory
            .provider(self.snapshot.as_ref())
            .current_index()
            .unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<StoreDataCache<S>>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn ledger_provider<'a>(
        &'a self,
        snapshot: &'a StoreDataCache<S>,
    ) -> impl LedgerProvider + ChainTipProvider + 'a {
        self.ledger_provider_factory.provider(snapshot)
    }

    fn native_contract_provider(&self) -> Option<Arc<Self::NativeProvider>> {
        Some(Arc::clone(&self.native_contract_provider))
    }

    fn requires_replay_artifacts(&self, block: &Block, context: BlockPersistContext) -> bool {
        self.hooks.requires_replay_artifacts(block, context)
    }

    fn block_committing(
        &self,
        block: &Block,
        snapshot: &StoreDataCache<S>,
        application_executed: &[ApplicationExecuted],
    ) -> bool {
        let accepted = self.hooks.block_committing(
            block,
            snapshot,
            application_executed,
            neo_runtime::sync_metrics::peer_live_tip(),
            BlockPersistContext::live(),
        );
        if !accepted {
            self.mark_fatal_persistence_error("block pre-commit observer rejected persistence");
        }
        accepted
    }

    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &StoreDataCache<S>,
        application_executed: &[ApplicationExecuted],
        context: BlockPersistContext,
    ) -> bool {
        let accepted = self.hooks.block_committing(
            block,
            snapshot,
            application_executed,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        );
        if !accepted {
            self.mark_fatal_persistence_error("block pre-commit observer rejected persistence");
        }
        accepted
    }

    fn commit_to_store(&self) -> Result<(), String> {
        let mut store_cache = self.store_cache.lock();
        let result = self.hooks.commit_canonical(&mut *store_cache);
        match &result {
            Ok(()) => self.hooks.canonical_commit_succeeded(),
            Err(error) => self.mark_fatal_persistence_error(error),
        }
        result
    }

    fn abort_store_commit(&self) {
        self.store_cache.lock().discard_pending_changes();
        self.mark_fatal_persistence_error("canonical store commit aborted");
    }

    fn should_stop_blockchain_service(&self) -> bool {
        self.fatal_persistence_error.load(Ordering::Acquire)
            || self.hooks.should_stop_blockchain_service()
    }

    fn sync_batch_commit_policy(
        &self,
        start_height: u32,
        end_height: u32,
    ) -> SyncBatchCommitPolicy {
        self.hooks.sync_batch_commit_policy(
            start_height,
            end_height,
            neo_runtime::sync_metrics::peer_live_tip(),
        )
    }

    fn flush_deferred_commit_handlers(&self) -> Result<(), String> {
        let result = self.hooks.flush_deferred();
        if let Err(error) = &result {
            self.mark_fatal_persistence_error(error);
        }
        result
    }

    fn deferred_import_work_budget(&self) -> Option<usize> {
        self.hooks.deferred_import_work_budget()
    }

    fn pending_deferred_import_work(&self) -> usize {
        self.hooks.pending_deferred_import_work()
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        self.hooks.allows_empty_block_fast_forward()
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        self.hooks.allows_empty_block_committing_fast_forward()
    }

    async fn block_finalized(
        &self,
        finalized: FinalizedBlock<Self::CacheBacking>,
    ) -> Result<(), String> {
        let result = self
            .hooks
            .block_finalized(finalized, neo_runtime::sync_metrics::peer_live_tip())
            .await;
        if let Err(error) = &result {
            self.mark_fatal_finalized_delivery_error(error);
        }
        result
    }
}

#[cfg(test)]
#[path = "../tests/composition/system_context.rs"]
mod tests;

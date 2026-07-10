//! Core blockchain system context and application commit-hook boundary.
//!
//! `NodeSystemContext` owns provider-neutral node mechanics: protocol settings,
//! the canonical store snapshot, durable commits, and the native-contract
//! provider. Application-specific StateService and indexing policy is supplied
//! through the statically dispatched `BlockCommitHooks` collaborator.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use neo_blockchain::{
    BlockPersistContext, ChainTipProvider, EmptyLedgerProvider, HotColdLedgerProviderFactory,
    LedgerProviderFactory, SystemContext,
};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::persistence::store::Store;
use neo_storage::persistence::{CacheRead, StoreCache, StoreCacheBacking, StoreDataCache};
use parking_lot::Mutex;

const LEDGER_TIP_PROVIDER_FACTORY: HotColdLedgerProviderFactory<EmptyLedgerProvider> =
    HotColdLedgerProviderFactory::new(EmptyLedgerProvider);

/// Application-owned behavior around a canonical block commit.
///
/// `B` is the concrete cache backing used by the composed store. The hook is a
/// generic collaborator so block import remains monomorphized and no callback
/// allocation or virtual dispatch enters the persistence path.
pub trait BlockCommitHooks<B>: Send + Sync + fmt::Debug
where
    B: CacheRead,
{
    /// Run pre-commit observers and return whether persistence may continue.
    fn block_committing(
        &self,
        _block: &Block,
        _snapshot: &neo_storage::DataCache<B>,
        _application_executed: &[ApplicationExecuted],
        _live_tip: u64,
        _context: BlockPersistContext,
    ) -> bool {
        true
    }

    /// Notify post-commit observers.
    fn block_committed(&self, _block: &Block, _live_tip: u64, _context: BlockPersistContext) {}

    /// Flush deferred work at a trusted bulk-import boundary.
    fn flush_bulk_sync(&self) -> Result<(), String> {
        Ok(())
    }

    /// Fences pre-commit observer stores before canonical Ledger durability.
    fn fence_precommit_durability(&self) -> Result<(), String> {
        Ok(())
    }

    /// Notify application recovery policy after the canonical durability fence.
    fn canonical_commit_succeeded(&self) {}

    /// Notify application recovery policy when canonical publication cannot
    /// safely complete after pre-commit observers may have persisted state.
    fn canonical_commit_failed(&self, _reason: &str) {}

    /// Whether application recovery policy has made the canonical writer fatal.
    fn should_stop_blockchain_service(&self) -> bool {
        false
    }

    /// Whether a bulk import may skip per-block commit hooks entirely.
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

impl<B> BlockCommitHooks<B> for NoopBlockCommitHooks where B: CacheRead {}

/// Provider-neutral context consumed by `neo-blockchain`.
pub struct NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: Store,
    H: BlockCommitHooks<StoreCacheBacking<S>>,
{
    settings: Arc<ProtocolSettings>,
    snapshot: Arc<StoreDataCache<S>>,
    store_cache: Mutex<StoreCache<S>>,
    native_contract_provider: Arc<P>,
    hooks: Arc<H>,
    fatal_persistence_error: AtomicBool,
}

impl<P, S, H> NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: Store,
    H: BlockCommitHooks<StoreCacheBacking<S>>,
{
    /// Compose a blockchain context over one canonical store cache.
    pub fn new(
        settings: Arc<ProtocolSettings>,
        snapshot: Arc<StoreDataCache<S>>,
        store_cache: StoreCache<S>,
        native_contract_provider: Arc<P>,
        hooks: Arc<H>,
    ) -> Self {
        Self {
            settings,
            snapshot,
            store_cache: Mutex::new(store_cache),
            native_contract_provider,
            hooks,
            fatal_persistence_error: AtomicBool::new(false),
        }
    }

    fn mark_fatal_persistence_error(&self, reason: &str) {
        self.fatal_persistence_error.store(true, Ordering::Release);
        self.hooks.canonical_commit_failed(reason);
    }
}

impl<P, S, H> fmt::Debug for NodeSystemContext<P, S, H>
where
    P: NativeContractProvider,
    S: Store,
    H: BlockCommitHooks<StoreCacheBacking<S>>,
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
    S: Store + 'static,
    H: BlockCommitHooks<StoreCacheBacking<S>> + 'static,
{
    type NativeProvider = P;
    type CacheBacking = StoreCacheBacking<S>;

    fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    fn current_height(&self) -> u32 {
        LEDGER_TIP_PROVIDER_FACTORY
            .provider(self.snapshot.as_ref())
            .current_index()
            .unwrap_or(0)
    }

    fn store_snapshot(&self) -> Option<Arc<StoreDataCache<S>>> {
        Some(Arc::clone(&self.snapshot))
    }

    fn native_contract_provider(&self) -> Option<Arc<Self::NativeProvider>> {
        Some(Arc::clone(&self.native_contract_provider))
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
        if let Err(error) = self.hooks.fence_precommit_durability() {
            self.store_cache.lock().discard_pending_changes();
            let error = format!("pre-commit durability fence failed: {error}");
            self.mark_fatal_persistence_error(&error);
            return Err(error);
        }
        let result = self
            .store_cache
            .lock()
            .try_commit_durable()
            .map_err(|error| error.to_string());
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

    fn flush_bulk_sync_commit_handlers(&self) -> Result<(), String> {
        let result = self.hooks.flush_bulk_sync();
        if let Err(error) = &result {
            self.mark_fatal_persistence_error(error);
        }
        result
    }

    fn allows_empty_block_fast_forward(&self) -> bool {
        self.hooks.allows_empty_block_fast_forward()
    }

    fn allows_empty_block_committing_fast_forward(&self) -> bool {
        self.hooks.allows_empty_block_committing_fast_forward()
    }

    fn block_committed(&self, block: &Block) {
        self.hooks.block_committed(
            block,
            neo_runtime::sync_metrics::peer_live_tip(),
            BlockPersistContext::live(),
        );
    }

    fn block_committed_with_context(&self, block: &Block, context: BlockPersistContext) {
        self.hooks
            .block_committed(block, neo_runtime::sync_metrics::peer_live_tip(), context);
    }
}

#[cfg(test)]
#[path = "../tests/composition/system_context.rs"]
mod tests;

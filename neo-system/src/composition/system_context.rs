//! Core blockchain system context and application commit-hook boundary.
//!
//! `NodeSystemContext` owns provider-neutral node mechanics: protocol settings,
//! the canonical store snapshot, durable commits, and the native-contract
//! provider. Application-specific StateService and indexing policy is supplied
//! through the statically dispatched `BlockCommitHooks` collaborator.

use std::fmt;
use std::sync::Arc;

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
        }
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
        self.hooks.block_committing(
            block,
            snapshot,
            application_executed,
            neo_runtime::sync_metrics::peer_live_tip(),
            BlockPersistContext::live(),
        )
    }

    fn block_committing_with_context(
        &self,
        block: &Block,
        snapshot: &StoreDataCache<S>,
        application_executed: &[ApplicationExecuted],
        context: BlockPersistContext,
    ) -> bool {
        self.hooks.block_committing(
            block,
            snapshot,
            application_executed,
            neo_runtime::sync_metrics::peer_live_tip(),
            context,
        )
    }

    fn commit_to_store(&self) {
        self.store_cache.lock().commit();
    }

    fn flush_bulk_sync_commit_handlers(&self) -> Result<(), String> {
        self.hooks.flush_bulk_sync()
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

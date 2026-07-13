//! Provider-neutral core node assembly.
//!
//! This module owns the mechanical construction shared by every full node:
//! one canonical store cache and snapshot, one mempool, one header cache, one
//! ledger context, one statically dispatched cold Ledger fallback, and the
//! blockchain command service that joins them. Process policy such as task
//! supervision, RPC, HSM credentials, and observability remains in the
//! application crate.

use std::sync::Arc;

use neo_blockchain::{
    BlockchainHandle, BlockchainService, HeaderCache, LedgerContext, OptionalStaticLedgerProvider,
};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_storage::persistence::{StoreCache, StoreDataCache, TransactionalStore};

use crate::{BlockCommitHooks, Node, NodeBuilder, NodeResult, NodeSystemContext};

type CoreBlockchainService<P, S, H> = BlockchainService<NodeSystemContext<P, S, H>, MemoryPool<P>>;

/// Required inputs for provider-neutral node construction.
///
/// Required collaborators are constructor parameters, so an incomplete core
/// graph cannot be represented. Optional validation policy remains a named
/// builder step.
pub struct NodeCoreBuilder<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    settings: Arc<ProtocolSettings>,
    storage: Arc<S>,
    native_contract_provider: Arc<P>,
    cold_ledger_provider: OptionalStaticLedgerProvider,
    hooks: Arc<H>,
    persisted_height: u32,
    stop_at_height: Option<u32>,
}

impl<P, S, H> NodeCoreBuilder<P, S, H>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
    H: BlockCommitHooks<S> + 'static,
{
    /// Start core composition from its required concrete collaborators.
    pub fn new(
        settings: Arc<ProtocolSettings>,
        storage: Arc<S>,
        native_contract_provider: Arc<P>,
        hooks: Arc<H>,
        persisted_height: u32,
    ) -> Self {
        Self {
            settings,
            storage,
            native_contract_provider,
            cold_ledger_provider: OptionalStaticLedgerProvider::default(),
            hooks,
            persisted_height,
            stop_at_height: None,
        }
    }

    /// Limit persistence to a validation/import target height.
    pub fn with_stop_at_height(mut self, stop_at_height: Option<u32>) -> Self {
        self.stop_at_height = stop_at_height;
        self
    }

    /// Install the immutable Ledger fallback selected by the application.
    pub fn with_cold_ledger_provider(
        mut self,
        cold_ledger_provider: OptionalStaticLedgerProvider,
    ) -> Self {
        self.cold_ledger_provider = cold_ledger_provider;
        self
    }

    /// Compose the core service and the handles needed by outer workflows.
    pub fn build(self) -> NodeCoreLaunch<P, S, H> {
        let store_cache = StoreCache::new_from_store(Arc::clone(&self.storage), false);
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let mempool = Arc::new(MemoryPool::new_with_native_contract_provider(
            &self.settings,
            Arc::clone(&self.native_contract_provider),
        ));
        let header_cache = Arc::new(HeaderCache::default());
        let ledger_context = Arc::new(LedgerContext::default());
        if self.persisted_height > 0 {
            ledger_context.record_tip(self.persisted_height);
        }

        let system_context = Arc::new(NodeSystemContext::new_with_ledger_provider(
            Arc::clone(&self.settings),
            Arc::clone(&snapshot),
            store_cache,
            Arc::clone(&self.native_contract_provider),
            self.cold_ledger_provider.clone(),
            self.hooks,
        ));
        let (mut service, blockchain) = BlockchainService::with_defaults(
            system_context,
            Arc::clone(&ledger_context),
            Arc::clone(&header_cache),
            Arc::clone(&mempool),
        );
        service.set_stop_at_height(self.stop_at_height);

        NodeCoreLaunch {
            core: NodeCore {
                settings: self.settings,
                storage: self.storage,
                blockchain,
                mempool,
                header_cache,
                ledger_context,
                snapshot,
                native_contract_provider: self.native_contract_provider,
                cold_ledger_provider: self.cold_ledger_provider,
                persisted_height: self.persisted_height,
            },
            blockchain_task: BlockchainTask { service },
        }
    }
}

/// Composed core before its blockchain command loop is handed to a supervisor.
pub struct NodeCoreLaunch<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    core: NodeCore<P, S>,
    blockchain_task: BlockchainTask<P, S, H>,
}

impl<P, S, H> NodeCoreLaunch<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    /// Separate shareable core handles from the owned service task.
    pub fn into_parts(self) -> (NodeCore<P, S>, BlockchainTask<P, S, H>) {
        (self.core, self.blockchain_task)
    }
}

/// Owned blockchain command loop awaiting application-level supervision.
pub struct BlockchainTask<P, S, H>
where
    P: NativeContractProvider,
    S: TransactionalStore,
    H: BlockCommitHooks<S>,
{
    service: CoreBlockchainService<P, S, H>,
}

impl<P, S, H> BlockchainTask<P, S, H>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
    H: BlockCommitHooks<S> + 'static,
{
    /// Drive blockchain commands until the handle requests shutdown.
    pub async fn run(self) {
        self.service.run().await;
    }
}

/// Shareable core handles used while the application wires outer services.
///
/// Fields stay private so callers select named capabilities. Consuming
/// [`NodeCore::into_node`] closes the composition stage and prevents a final
/// node from being assembled with a different store, mempool, or provider.
pub struct NodeCore<P, S>
where
    P: NativeContractProvider,
    S: TransactionalStore,
{
    settings: Arc<ProtocolSettings>,
    storage: Arc<S>,
    blockchain: BlockchainHandle,
    mempool: Arc<MemoryPool<P>>,
    header_cache: Arc<HeaderCache>,
    ledger_context: Arc<LedgerContext>,
    snapshot: Arc<StoreDataCache<S>>,
    native_contract_provider: Arc<P>,
    cold_ledger_provider: OptionalStaticLedgerProvider,
    persisted_height: u32,
}

impl<P, S> NodeCore<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    /// Height represented by the durable store when this core was composed.
    pub fn persisted_height(&self) -> u32 {
        self.persisted_height
    }

    /// Canonical blockchain command handle.
    pub fn blockchain(&self) -> BlockchainHandle {
        self.blockchain.clone()
    }

    /// Canonical write-back snapshot shared with block persistence.
    pub fn snapshot(&self) -> Arc<StoreDataCache<S>> {
        Arc::clone(&self.snapshot)
    }

    /// In-memory ledger context shared with block persistence and P2P reads.
    pub fn ledger_context(&self) -> Arc<LedgerContext> {
        Arc::clone(&self.ledger_context)
    }

    /// Shared transaction pool used by admission, consensus, P2P, and RPC.
    pub fn mempool(&self) -> Arc<MemoryPool<P>> {
        Arc::clone(&self.mempool)
    }

    /// Finish composition with the already-launched network handle.
    pub fn into_node(self, network: NetworkHandle) -> NodeResult<Node<P, S>> {
        NodeBuilder::default()
            .with_settings(self.settings)
            .with_storage(self.storage)
            .with_blockchain(self.blockchain)
            .with_network(network)
            .with_mempool(self.mempool)
            .with_header_cache(self.header_cache)
            .with_native_contract_provider(self.native_contract_provider)
            .with_cold_ledger_provider(self.cold_ledger_provider)
            .build()
    }
}

#[cfg(test)]
#[path = "../tests/composition/core.rs"]
mod tests;

//! Final assembly of an already-composed Neo node.
//!
//! [`NodeCoreBuilder`](super::core::NodeCoreBuilder) owns provider-neutral
//! service construction. This builder only joins the resulting, required
//! handles with the application-owned network handle. Required components are
//! constructor arguments, so an incomplete node graph cannot be represented.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, HeaderCache, OptionalStaticLedgerProvider};
use neo_config::NeoChainSpec;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_runtime::{SharedStoreSyncStageCheckpointStore, SharedStoreVerifiedHeaderStore};
use neo_storage::persistence::TransactionalStore;
use neo_storage::persistence::providers::MemoryStore;

use crate::live_block_import_pipeline::LiveBlockImportPipeline;
use crate::node::Node;
use crate::staged_sync_pipeline::StagedSyncPipeline;
use crate::wallet_provider::WalletProvider;

/// Fluent final assembly for [`Node`].
pub struct NodeBuilder<P = neo_native_contracts::StandardNativeProvider, S = MemoryStore>
where
    P: NativeContractProvider,
    S: TransactionalStore,
{
    chain_spec: Arc<NeoChainSpec>,
    storage: Arc<S>,
    blockchain: BlockchainHandle,
    network: NetworkHandle,
    mempool: Arc<MemoryPool<P>>,
    header_cache: Arc<HeaderCache>,
    native_contract_provider: Arc<P>,
    wallets: WalletProvider,
    cold_ledger_provider: OptionalStaticLedgerProvider,
    staged_sync_pipeline: Option<
        Arc<
            StagedSyncPipeline<
                SharedStoreSyncStageCheckpointStore<S>,
                SharedStoreVerifiedHeaderStore<S>,
            >,
        >,
    >,
}

impl<P, S> NodeBuilder<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    /// Starts final assembly with every required runtime capability.
    #[must_use]
    pub fn new(
        chain_spec: Arc<NeoChainSpec>,
        storage: Arc<S>,
        blockchain: BlockchainHandle,
        network: NetworkHandle,
        mempool: Arc<MemoryPool<P>>,
        header_cache: Arc<HeaderCache>,
        native_contract_provider: Arc<P>,
    ) -> Self {
        Self {
            chain_spec,
            storage,
            blockchain,
            network,
            mempool,
            header_cache,
            native_contract_provider,
            wallets: WalletProvider::default(),
            cold_ledger_provider: OptionalStaticLedgerProvider::default(),
            staged_sync_pipeline: None,
        }
    }

    /// Installs the optional wallet provider.
    #[must_use]
    pub fn with_wallets(mut self, wallets: WalletProvider) -> Self {
        self.wallets = wallets;
        self
    }

    /// Installs the optional immutable Ledger fallback.
    #[must_use]
    pub fn with_cold_ledger_provider(
        mut self,
        cold_ledger_provider: OptionalStaticLedgerProvider,
    ) -> Self {
        self.cold_ledger_provider = cold_ledger_provider;
        self
    }

    /// Installs a staged-sync pipeline that was composed over these exact
    /// storage, blockchain, and header-cache handles.
    #[must_use]
    pub fn with_staged_sync_pipeline(
        mut self,
        pipeline: Arc<
            StagedSyncPipeline<
                SharedStoreSyncStageCheckpointStore<S>,
                SharedStoreVerifiedHeaderStore<S>,
            >,
        >,
    ) -> Self {
        self.staged_sync_pipeline = Some(pipeline);
        self
    }

    /// Completes final node assembly.
    #[must_use]
    pub fn build(self) -> Node<P, S> {
        let staged_sync_pipeline = self.staged_sync_pipeline.unwrap_or_else(|| {
            Arc::new(StagedSyncPipeline::new(
                self.blockchain.clone(),
                Arc::clone(&self.header_cache),
                Arc::clone(&self.storage),
            ))
        });
        let live_block_import_pipeline = Arc::new(LiveBlockImportPipeline::new(
            self.blockchain.clone(),
            staged_sync_pipeline.import().import_queue(),
        ));

        Node {
            chain_spec: self.chain_spec,
            storage: self.storage,
            wallets: self.wallets,
            blockchain: self.blockchain,
            network: self.network,
            staged_sync_pipeline,
            live_block_import_pipeline,
            mempool: self.mempool,
            header_cache: self.header_cache,
            native_contract_provider: self.native_contract_provider,
            ledger_provider_factory: neo_blockchain::HotColdLedgerProviderFactory::new(
                self.cold_ledger_provider,
            ),
        }
    }
}

impl<P, S> std::fmt::Debug for NodeBuilder<P, S>
where
    P: NativeContractProvider + 'static,
    S: TransactionalStore + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeBuilder")
            .field("chain_spec", &self.chain_spec.identity())
            .field("storage", &"<Store>")
            .field("blockchain", &"BlockchainHandle")
            .field("network", &"NetworkHandle")
            .field("mempool", &self.mempool.total_count())
            .field("header_cache", &self.header_cache.count())
            .field(
                "native_contract_provider_contracts",
                &self.native_contract_provider.all_native_contracts().len(),
            )
            .field("wallets", &self.wallets)
            .field(
                "cold_ledger_provider",
                &self.cold_ledger_provider.is_enabled(),
            )
            .field("staged_sync_pipeline", &self.staged_sync_pipeline.is_some())
            .finish()
    }
}

#[cfg(test)]
#[path = "../tests/composition/builder.rs"]
mod tests;

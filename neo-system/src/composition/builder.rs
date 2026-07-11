//! Fluent builder for [`crate::Node`].
//!
//! All fields are `Option<...>` so an unset service is caught at
//! [`NodeBuilder::build`] time with a descriptive error. There is
//! no `Option::unwrap` in any public method, so a partially
//! configured builder is always safe to drop.
//!
//! The required components — storage plus the blockchain and network handles —
//! are validated at [`NodeBuilder::build`], which null-checks each concrete
//! field and returns a descriptive missing-service / missing-config error when
//! one is absent. There are no trait-object executor / consensus / engine
//! fields to compose: those were removed in ADR-032 / ADR-033. The native
//! contract provider is an explicit composition-root dependency; callers must
//! pass the same provider that block import, RPC, consensus, and mempool
//! admission use. The optional static Ledger fallback is also captured once
//! and retained by the final node, so historical reads do not reconstruct
//! runtime policy locally. The sync import pipeline is built by default from
//! the same blockchain and storage handles, so staged-sync callers use one
//! explicit import/checkpoint boundary.

use std::sync::Arc;
use tracing::debug;

use neo_blockchain::{BlockchainHandle, HeaderCache, OptionalStaticLedgerProvider};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_runtime::SharedStoreSyncStageCheckpointStore;
use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::store::Store;

use crate::error::NodeResult;
use crate::node::Node;
use crate::sync_import_pipeline::SyncImportPipeline;
use crate::wallet_provider::WalletProvider;

/// Fluent builder for [`Node`].
pub struct NodeBuilder<P = neo_native_contracts::StandardNativeProvider, S = MemoryStore>
where
    P: NativeContractProvider,
    S: Store,
{
    settings: Option<Arc<ProtocolSettings>>,
    storage: Option<Arc<S>>,
    wallets: Option<WalletProvider>,
    blockchain: Option<BlockchainHandle>,
    network: Option<NetworkHandle>,
    mempool: Option<Arc<MemoryPool<P>>>,
    header_cache: Option<Arc<HeaderCache>>,
    native_contract_provider: Option<Arc<P>>,
    cold_ledger_provider: OptionalStaticLedgerProvider,
    sync_import_pipeline: Option<Arc<SyncImportPipeline<SharedStoreSyncStageCheckpointStore<S>>>>,
}

impl<P, S> Default for NodeBuilder<P, S>
where
    P: NativeContractProvider,
    S: Store,
{
    fn default() -> Self {
        Self {
            settings: None,
            storage: None,
            wallets: None,
            blockchain: None,
            network: None,
            mempool: None,
            header_cache: None,
            native_contract_provider: None,
            cold_ledger_provider: OptionalStaticLedgerProvider::default(),
            sync_import_pipeline: None,
        }
    }
}

impl<P, S> std::fmt::Debug for NodeBuilder<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeBuilder")
            .field("settings", &self.settings.is_some())
            .field("storage", &self.storage.is_some())
            .field("wallets", &self.wallets.is_some())
            .field("blockchain", &self.blockchain.is_some())
            .field("network", &self.network.is_some())
            .field("mempool", &self.mempool.is_some())
            .field("header_cache", &self.header_cache.is_some())
            .field(
                "native_contract_provider",
                &self.native_contract_provider.is_some(),
            )
            .field(
                "cold_ledger_provider",
                &self.cold_ledger_provider.is_enabled(),
            )
            .field("sync_import_pipeline", &self.sync_import_pipeline.is_some())
            .finish()
    }
}

impl<P, S> NodeBuilder<P, S>
where
    P: NativeContractProvider + 'static,
    S: Store + 'static,
{
    /// Install the protocol settings.
    pub fn with_settings(mut self, settings: Arc<ProtocolSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Install the storage backend.
    pub fn with_storage(mut self, storage: Arc<S>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Install the wallet provider.
    pub fn with_wallets(mut self, wallets: WalletProvider) -> Self {
        self.wallets = Some(wallets);
        self
    }

    /// Install the blockchain service handle.
    pub fn with_blockchain(mut self, blockchain: BlockchainHandle) -> Self {
        self.blockchain = Some(blockchain);
        self
    }

    /// Install the network service handle.
    pub fn with_network(mut self, network: NetworkHandle) -> Self {
        self.network = Some(network);
        self
    }

    /// Install a shared memory pool. When unset, [`Self::build`] constructs a
    /// fresh pool from the protocol settings and the explicit native-contract
    /// provider. Pass the same `Arc` the blockchain service admits into so RPC
    /// reads see the live pool.
    pub fn with_mempool(mut self, mempool: Arc<MemoryPool<P>>) -> Self {
        self.mempool = Some(mempool);
        self
    }

    /// Install a shared header cache. When unset, [`Self::build`]
    /// constructs an empty cache. Pass the same `Arc` the blockchain
    /// service appends to so RPC header queries see the live cache.
    pub fn with_header_cache(mut self, header_cache: Arc<HeaderCache>) -> Self {
        self.header_cache = Some(header_cache);
        self
    }

    /// Sets the native-contract provider used by NeoVM host calls.
    ///
    /// The provider is required. Composition roots should create one provider
    /// and pass the same `Arc` into block import, RPC, consensus, and mempool
    /// admission so native dispatch has one visible owner.
    pub fn with_native_contract_provider(mut self, provider: Arc<P>) -> Self {
        self.native_contract_provider = Some(provider);
        self
    }

    /// Install the immutable Ledger fallback opened by the application.
    pub fn with_cold_ledger_provider(
        mut self,
        cold_ledger_provider: OptionalStaticLedgerProvider,
    ) -> Self {
        self.cold_ledger_provider = cold_ledger_provider;
        self
    }

    /// Install a pre-composed sync import pipeline.
    ///
    /// When unset, [`Self::build`] creates one over the same blockchain handle
    /// and storage provider installed on the node.
    pub fn with_sync_import_pipeline(
        mut self,
        pipeline: Arc<SyncImportPipeline<SharedStoreSyncStageCheckpointStore<S>>>,
    ) -> Self {
        self.sync_import_pipeline = Some(pipeline);
        self
    }

    /// Finalise the builder.
    pub fn build(self) -> NodeResult<Node<P, S>> {
        let settings = self
            .settings
            .ok_or_else(|| crate::error::NodeError::missing_config("settings"))?;
        let storage = self
            .storage
            .ok_or_else(|| crate::error::NodeError::missing_config("storage"))?;
        let blockchain = self
            .blockchain
            .ok_or_else(|| crate::error::NodeError::missing_service("blockchain"))?;
        let network = self
            .network
            .ok_or_else(|| crate::error::NodeError::missing_service("network"))?;
        let native_contract_provider = self
            .native_contract_provider
            .ok_or_else(|| crate::error::NodeError::missing_service("native_contract_provider"))?;

        debug!("NodeBuilder::build: composing runtime node");
        let mempool = self.mempool.unwrap_or_else(|| {
            Arc::new(MemoryPool::new_with_native_contract_provider(
                &settings,
                Arc::clone(&native_contract_provider),
            ))
        });
        let sync_import_pipeline = self.sync_import_pipeline.unwrap_or_else(|| {
            Arc::new(SyncImportPipeline::new(
                blockchain.clone(),
                Arc::clone(&storage),
            ))
        });
        Ok(Node {
            settings,
            storage,
            wallets: self.wallets.unwrap_or_default(),
            blockchain,
            network,
            sync_import_pipeline,
            mempool,
            header_cache: self.header_cache.unwrap_or_default(),
            native_contract_provider,
            ledger_provider_factory: neo_blockchain::HotColdLedgerProviderFactory::new(
                self.cold_ledger_provider,
            ),
        })
    }
}

#[cfg(test)]
#[path = "../tests/composition/builder.rs"]
mod tests;

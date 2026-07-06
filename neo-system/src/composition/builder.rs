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
//! contract provider can be supplied explicitly by a composition root; when it
//! is not supplied, the builder owns the standard Neo N3 provider locally. The
//! sync import pipeline is also built by default from the same blockchain and
//! storage handles so staged-sync callers can use one shared import/checkpoint
//! boundary.

use std::sync::Arc;
use tracing::debug;

use neo_blockchain::{BlockchainHandle, HeaderCache};
use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_mempool::MemoryPool;
use neo_network::NetworkHandle;
use neo_storage::persistence::store::Store;

use crate::error::NodeResult;
use crate::node::Node;
use crate::sync_import_pipeline::SyncImportPipeline;
use crate::wallet_provider::WalletProvider;
use neo_runtime::ServiceRegistry;

/// Fluent builder for [`Node`].
#[derive(Default)]
pub struct NodeBuilder {
    settings: Option<Arc<ProtocolSettings>>,
    storage: Option<Arc<dyn Store>>,
    wallets: Option<WalletProvider>,
    blockchain: Option<BlockchainHandle>,
    network: Option<NetworkHandle>,
    mempool: Option<Arc<MemoryPool>>,
    header_cache: Option<Arc<HeaderCache>>,
    services: Option<ServiceRegistry>,
    native_contract_provider: Option<Arc<dyn NativeContractProvider>>,
    sync_import_pipeline: Option<Arc<SyncImportPipeline>>,
}

impl std::fmt::Debug for NodeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeBuilder")
            .field("settings", &self.settings.is_some())
            .field("storage", &self.storage.is_some())
            .field("wallets", &self.wallets.is_some())
            .field("blockchain", &self.blockchain.is_some())
            .field("network", &self.network.is_some())
            .field("mempool", &self.mempool.is_some())
            .field("header_cache", &self.header_cache.is_some())
            .field("services", &self.services.is_some())
            .field(
                "native_contract_provider",
                &self.native_contract_provider.is_some(),
            )
            .field("sync_import_pipeline", &self.sync_import_pipeline.is_some())
            .finish()
    }
}

impl NodeBuilder {
    /// Install the protocol settings.
    pub fn with_settings(mut self, settings: Arc<ProtocolSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Install the storage backend.
    pub fn with_storage(mut self, storage: Arc<dyn Store>) -> Self {
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

    /// Install a shared memory pool. When unset, [`Self::build`]
    /// constructs a fresh pool from the protocol settings. Pass the
    /// same `Arc` the blockchain service admits into so RPC reads see
    /// the live pool.
    pub fn with_mempool(mut self, mempool: Arc<MemoryPool>) -> Self {
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

    /// Install a pre-populated service registry. When unset,
    /// [`Self::build`] starts with an empty registry; services can
    /// also be registered after `build()` via
    /// [`Node::register_service`].
    pub fn with_services(mut self, services: ServiceRegistry) -> Self {
        self.services = Some(services);
        self
    }

    /// Sets the native-contract provider used by NeoVM host calls.
    ///
    /// When unset, [`Self::build`] uses the standard Neo N3 provider from
    /// `neo-native-contracts`. Supplying a provider here makes native dispatch
    /// an explicit composition-root dependency.
    pub fn with_native_contract_provider(
        mut self,
        provider: Arc<dyn NativeContractProvider>,
    ) -> Self {
        self.native_contract_provider = Some(provider);
        self
    }

    /// Install a pre-composed sync import pipeline.
    ///
    /// When unset, [`Self::build`] creates one over the same blockchain handle
    /// and storage provider installed on the node.
    pub fn with_sync_import_pipeline(mut self, pipeline: Arc<SyncImportPipeline>) -> Self {
        self.sync_import_pipeline = Some(pipeline);
        self
    }

    /// Finalise the builder.
    pub fn build(self) -> NodeResult<Node> {
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
        let native_contract_provider = self.native_contract_provider.unwrap_or_else(|| {
            Arc::new(neo_native_contracts::StandardNativeProvider::new())
                as Arc<dyn NativeContractProvider>
        });

        debug!("NodeBuilder::build: composing runtime node");
        let mempool = self
            .mempool
            .unwrap_or_else(|| Arc::new(MemoryPool::new(&settings)));
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
            services: self.services.unwrap_or_default(),
            native_contract_provider,
            shutdown: tokio_util::sync::CancellationToken::new(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/composition/builder.rs"]
mod tests;

//! Fluent builder for [`crate::Node`].
//!
//! All fields are `Option<...>` so an unset service is caught at
//! [`NodeBuilder::build`] time with a descriptive error. There is
//! no `Option::unwrap` in any public method, so a partially
//! configured builder is always safe to drop.
//!
//! The trait-object services (block executor, consensus, engine
//! API) stay `Option<...>` because their concrete `impl`s are
//! still landing in subsequent stages. The handles
//! (`BlockchainHandle`, `NetworkHandle`) and the storage backend
//! are required.

use std::sync::Arc;
use tracing::debug;

use neo_blockchain::BlockchainHandle;
use neo_config::ProtocolSettings;
use neo_network::NetworkHandle;
use neo_runtime::{BlockExecutor, ConsensusService, NeoEngine};
use neo_storage::persistence::store::Store;

use crate::error::NodeResult;
use crate::node::Node;
use crate::wallet_provider::WalletProvider;

/// Fluent builder for [`Node`].
#[derive(Default)]
pub struct NodeBuilder {
    settings: Option<Arc<ProtocolSettings>>,
    storage: Option<Arc<dyn Store>>,
    wallets: Option<WalletProvider>,
    blockchain: Option<BlockchainHandle>,
    network: Option<NetworkHandle>,
    block_executor: Option<Arc<dyn BlockExecutor>>,
    consensus: Option<Arc<dyn ConsensusService>>,
    engine: Option<Arc<dyn NeoEngine>>,
}

impl std::fmt::Debug for NodeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeBuilder")
            .field("settings", &self.settings.is_some())
            .field("storage", &self.storage.is_some())
            .field("wallets", &self.wallets.is_some())
            .field("blockchain", &self.blockchain.is_some())
            .field("network", &self.network.is_some())
            .field("block_executor", &self.block_executor.as_ref().map(|s| s.name()))
            .field("consensus", &self.consensus.as_ref().map(|s| s.name()))
            .field("engine", &self.engine.as_ref().map(|s| s.name()))
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

    /// Install a block executor service.
    pub fn with_block_executor(mut self, executor: Arc<dyn BlockExecutor>) -> Self {
        self.block_executor = Some(executor);
        self
    }

    /// Install a consensus service.
    pub fn with_consensus(mut self, consensus: Arc<dyn ConsensusService>) -> Self {
        self.consensus = Some(consensus);
        self
    }

    /// Install an engine API service.
    pub fn with_engine(mut self, engine: Arc<dyn NeoEngine>) -> Self {
        self.engine = Some(engine);
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

        debug!("NodeBuilder::build: composing runtime node");
        Ok(Node {
            settings,
            storage,
            wallets: self.wallets.unwrap_or_default(),
            blockchain,
            network,
            block_executor: self.block_executor,
            consensus: self.consensus,
            engine: self.engine,
            shutdown: tokio_util::sync::CancellationToken::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    fn memory_store() -> Arc<dyn Store> {
        Arc::new(MemoryStore::new())
    }

    #[test]
    fn builder_requires_settings() {
        let result = NodeBuilder::default().build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_requires_storage() {
        let result = NodeBuilder::default()
            .with_settings(Arc::new(ProtocolSettings::default()))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_requires_blockchain_and_network() {
        let result = NodeBuilder::default()
            .with_settings(Arc::new(ProtocolSettings::default()))
            .with_storage(memory_store())
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_succeeds_with_required_services() {
        let storage = memory_store();
        let settings = Arc::new(ProtocolSettings::default());
        let (bc, _rx) = BlockchainHandle::with_capacity();
        let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);

        let node = NodeBuilder::default()
            .with_settings(settings)
            .with_storage(storage)
            .with_blockchain(bc)
            .with_network(net)
            .build()
            .expect("required services set");
        assert!(node.block_executor.is_none());
        assert!(node.consensus.is_none());
        assert!(node.engine.is_none());
    }
}

//! Node-level composition of the reth-style services.
//!
//! The [`Node`] struct is what the node binary actually constructs at
//! startup: a single owner of all long-running services, each stored as
//! an `Arc<dyn ServiceTrait>`. Consumers (RPC server, consensus driver,
//! plugins) receive a `Clone` of the [`Node`] and call into the
//! services directly through the trait objects.
//!
//! Construction goes through [`NodeBuilder`] so the *combination* of
//! services is type-checked at `build()` time, but every individual
//! service is set up through a fluent `with_*` call. This matches the
//! reth `NodeBuilder` pattern: build phase validates, run phase is
//! cheap.

use std::sync::Arc;

use crate::blockchain::BlockchainHandle;
use crate::errors::{ServiceError, ServiceResult};
use crate::services::{
    BlockExecutor, ConsensusService, MempoolService, NeoEngine, NetworkService,
};

/// Container for every service the runtime exposes to the rest of the
/// workspace.
///
/// The fields are public so the call site can write
/// `node.mempool.add_transaction(tx).await?` without going through an
/// accessor method. Each field is either a `Clone`-able
/// `Arc<dyn ServiceTrait>` (the trait-object services) or a
/// `Clone`-able command handle ([`BlockchainHandle`]).
#[derive(Clone)]
pub struct Node {
    /// Block execution / validation service. See [`BlockExecutor`].
    pub block_executor: Arc<dyn BlockExecutor>,
    /// Transaction pool. See [`MempoolService`].
    pub mempool: Arc<dyn MempoolService>,
    /// P2P networking. See [`NetworkService`].
    pub network: Arc<dyn NetworkService>,
    /// dBFT consensus driver. See [`ConsensusService`].
    pub consensus: Arc<dyn ConsensusService>,
    /// Engine API. See [`NeoEngine`].
    pub engine: Arc<dyn NeoEngine>,
    /// Blockchain command / event handle. See [`BlockchainHandle`].
    pub blockchain: BlockchainHandle,
}

impl std::fmt::Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Node")
            .field("block_executor", &self.block_executor.name())
            .field("mempool", &self.mempool.name())
            .field("network", &self.network.name())
            .field("consensus", &self.consensus.name())
            .field("engine", &self.engine.name())
            .field("blockchain", &"BlockchainHandle")
            .finish()
    }
}

impl Node {
    /// Returns a fresh [`NodeBuilder`].
    pub fn builder() -> NodeBuilder {
        NodeBuilder::default()
    }
}

/// Fluent builder for [`Node`].
///
/// All fields are `Option<Arc<dyn Trait>>` so an unset service is
/// caught at [`NodeBuilder::build`] time with a descriptive error.
/// There is no `Option::unwrap` in any public method, so a partially
/// configured builder is always safe to drop.
#[derive(Default)]
pub struct NodeBuilder {
    block_executor: Option<Arc<dyn BlockExecutor>>,
    mempool: Option<Arc<dyn MempoolService>>,
    network: Option<Arc<dyn NetworkService>>,
    consensus: Option<Arc<dyn ConsensusService>>,
    engine: Option<Arc<dyn NeoEngine>>,
    blockchain: Option<BlockchainHandle>,
}

impl std::fmt::Debug for NodeBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeBuilder")
            .field("block_executor", &self.block_executor.as_ref().map(|s| s.name()))
            .field("mempool", &self.mempool.as_ref().map(|s| s.name()))
            .field("network", &self.network.as_ref().map(|s| s.name()))
            .field("consensus", &self.consensus.as_ref().map(|s| s.name()))
            .field("engine", &self.engine.as_ref().map(|s| s.name()))
            .field("blockchain", &self.blockchain.is_some())
            .finish()
    }
}

impl NodeBuilder {
    /// Install the block executor service.
    pub fn with_block_executor(mut self, executor: Arc<dyn BlockExecutor>) -> Self {
        self.block_executor = Some(executor);
        self
    }

    /// Install the mempool service.
    pub fn with_mempool(mut self, mempool: Arc<dyn MempoolService>) -> Self {
        self.mempool = Some(mempool);
        self
    }

    /// Install the network service.
    pub fn with_network(mut self, network: Arc<dyn NetworkService>) -> Self {
        self.network = Some(network);
        self
    }

    /// Install the consensus service.
    pub fn with_consensus(mut self, consensus: Arc<dyn ConsensusService>) -> Self {
        self.consensus = Some(consensus);
        self
    }

    /// Install the engine service.
    pub fn with_engine(mut self, engine: Arc<dyn NeoEngine>) -> Self {
        self.engine = Some(engine);
        self
    }

    /// Install the blockchain command / event handle.
    pub fn with_blockchain(mut self, blockchain: BlockchainHandle) -> Self {
        self.blockchain = Some(blockchain);
        self
    }

    /// Finalise the builder. Returns [`ServiceError::InvalidState`] if
    /// any required service has not been set.
    pub fn build(self) -> ServiceResult<Node> {
        let block_executor = self
            .block_executor
            .ok_or_else(|| ServiceError::invalid_state("block_executor service not set"))?;
        let mempool = self
            .mempool
            .ok_or_else(|| ServiceError::invalid_state("mempool service not set"))?;
        let network = self
            .network
            .ok_or_else(|| ServiceError::invalid_state("network service not set"))?;
        let consensus = self
            .consensus
            .ok_or_else(|| ServiceError::invalid_state("consensus service not set"))?;
        let engine = self
            .engine
            .ok_or_else(|| ServiceError::invalid_state("engine service not set"))?;
        let blockchain = self
            .blockchain
            .ok_or_else(|| ServiceError::invalid_state("blockchain handle not set"))?;

        Ok(Node {
            block_executor,
            mempool,
            network,
            consensus,
            engine,
            blockchain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Service;
    use crate::blockchain::BlockchainHandle;
    use crate::outcome::ExecutionOutcome;
    use async_trait::async_trait;
    use neo_payloads::Block;
    use tokio::sync::broadcast;

    #[derive(Debug)]
    struct NoopExecutor;
    impl Service for NoopExecutor {}
    #[async_trait]
    impl BlockExecutor for NoopExecutor {
        async fn execute(&self, _block: &Block) -> Result<ExecutionOutcome, ServiceError> {
            Ok(ExecutionOutcome::default())
        }
        async fn validate(&self, _block: &Block) -> Result<(), ServiceError> {
            Ok(())
        }
    }

    #[derive(Debug)]
    struct NoopMempool;
    impl Service for NoopMempool {}
    #[async_trait]
    impl MempoolService for NoopMempool {
        async fn add_transaction(&self, _tx: neo_payloads::Transaction) -> Result<crate::services::TxHash, ServiceError> {
            Ok(neo_primitives::UInt256::default())
        }
        async fn get_transactions(&self, _max: usize) -> Result<Vec<neo_payloads::Transaction>, ServiceError> {
            Ok(Vec::new())
        }
        async fn remove_transaction(&self, _hash: &neo_primitives::UInt256) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn count(&self) -> usize {
            0
        }
    }

    #[derive(Debug)]
    struct NoopNetwork;
    impl Service for NoopNetwork {}
    #[async_trait]
    impl NetworkService for NoopNetwork {
        async fn broadcast_block(&self, _block: &Block) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn broadcast_transaction(&self, _tx: &neo_payloads::Transaction) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn peer_count(&self) -> usize {
            0
        }
        fn subscribe_events(&self) -> broadcast::Receiver<crate::outcome::NetworkEvent> {
            let (_tx, rx) = broadcast::channel(1);
            rx
        }
    }

    #[derive(Debug)]
    struct NoopConsensus;
    impl Service for NoopConsensus {}
    #[async_trait]
    impl ConsensusService for NoopConsensus {
        async fn start(&self) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn stop(&self) -> Result<(), ServiceError> {
            Ok(())
        }
        async fn is_running(&self) -> bool {
            false
        }
    }

    #[derive(Debug)]
    struct NoopEngine;
    impl Service for NoopEngine {}
    #[async_trait]
    impl NeoEngine for NoopEngine {
        async fn execute_block(&self, _block: &Block) -> Result<crate::outcome::ExecutionPayload, ServiceError> {
            Ok(crate::outcome::ExecutionPayload::default())
        }
        async fn validate_block(&self, _block: &Block) -> Result<crate::outcome::ValidationResult, ServiceError> {
            Ok(crate::outcome::ValidationResult::ok())
        }
    }

    #[test]
    fn builder_requires_every_service() {
        let err = Node::builder().build().expect_err("missing services");
        assert!(matches!(err, ServiceError::InvalidState(_)));
    }

    #[tokio::test]
    async fn builder_succeeds_with_all_services() {
        let (blockchain, _rx) = BlockchainHandle::with_capacity();
        let node = Node::builder()
            .with_block_executor(Arc::new(NoopExecutor))
            .with_mempool(Arc::new(NoopMempool))
            .with_network(Arc::new(NoopNetwork))
            .with_consensus(Arc::new(NoopConsensus))
            .with_engine(Arc::new(NoopEngine))
            .with_blockchain(blockchain)
            .build()
            .expect("all services set");
        assert_eq!(node.mempool.count().await, 0);
        assert_eq!(node.network.peer_count().await, 0);
        assert!(!node.consensus.is_running().await);
    }
}

//! Neo Node Runtime
//!
//! This module provides the main runtime for the Neo N3 node, integrating:
//! - World state management (neo-state)
//! - Chain state controller (neo-chain)
//! - Transaction mempool (neo-mempool)
//! - P2P networking (neo-p2p)
//! - dBFT consensus (neo-consensus)
//! - State root calculation (neo-core state_service)
//! - Block execution (executor module)
//!
//! Communication between components uses pure tokio channels for best performance.

mod config;
mod events;
mod handlers;

pub use config::{
    RuntimeConfig, CHAIN_CHANNEL_SIZE, CONSENSUS_CHANNEL_SIZE, P2P_CHANNEL_SIZE,
    SHUTDOWN_CHANNEL_SIZE,
};
pub use events::RuntimeEvent;

use crate::executor::BlockExecutorImpl;
use crate::genesis::create_genesis_block;
use crate::state_validator::{StateRootValidator, StateValidatorConfig};
use neo_chain::{BlockIndexEntry, ChainEvent, ChainState};
use neo_consensus::{ConsensusEvent, ConsensusService};
use neo_core::neo_io::SerializableExt;
use neo_core::network::p2p::payloads::Block;
use neo_core::persistence::data_cache::DataCache;
use neo_core::state_service::{StateRoot, StateStore};
use neo_mempool::Mempool;
use neo_p2p::P2PEvent;
use neo_state::{MemoryWorldState, StateTrieManager, WorldState};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

/// The main node runtime
#[allow(dead_code)] // Fields will be used when full runtime integration is implemented
pub struct NodeRuntime {
    /// Runtime configuration
    config: RuntimeConfig,
    /// World state
    state: Arc<RwLock<MemoryWorldState>>,
    /// Chain state controller
    chain: Arc<RwLock<ChainState>>,
    /// Transaction mempool
    mempool: Arc<RwLock<Mempool>>,
    /// Consensus service (optional, only for validators)
    consensus: Arc<RwLock<Option<ConsensusService>>>,
    /// State store for state root calculation (optional)
    state_store: Option<Arc<StateStore>>,
    /// State trie manager for MPT-based state root calculation
    state_trie: Arc<RwLock<StateTrieManager>>,
    /// State root validator for network validation (optional)
    state_validator: Option<Arc<StateRootValidator>>,
    /// Block executor for transaction processing
    block_executor: Arc<BlockExecutorImpl>,
    /// Channel for chain events
    chain_tx: broadcast::Sender<ChainEvent>,
    /// Channel for consensus events
    consensus_tx: mpsc::Sender<ConsensusEvent>,
    consensus_rx: Option<mpsc::Receiver<ConsensusEvent>>,
    /// Channel for P2P events
    p2p_tx: mpsc::Sender<P2PEvent>,
    p2p_rx: Option<mpsc::Receiver<P2PEvent>>,
    /// Broadcast channel for runtime events
    event_tx: broadcast::Sender<RuntimeEvent>,
    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
    /// Running flag
    running: Arc<std::sync::atomic::AtomicBool>,
    /// P2P broadcast sender for consensus messages
    p2p_broadcast_tx: Option<broadcast::Sender<crate::p2p_service::BroadcastMessage>>,
}

impl NodeRuntime {
    /// Creates a new node runtime with the given configuration
    pub fn new(config: RuntimeConfig) -> Self {
        // Create channels
        let (chain_tx, _) = broadcast::channel(CHAIN_CHANNEL_SIZE);
        let (consensus_tx, consensus_rx) = mpsc::channel(CONSENSUS_CHANNEL_SIZE);
        let (p2p_tx, p2p_rx) = mpsc::channel(P2P_CHANNEL_SIZE);
        let (event_tx, _) = broadcast::channel(256);
        let (shutdown_tx, _) = broadcast::channel(SHUTDOWN_CHANNEL_SIZE);

        // Create world state
        let state = Arc::new(RwLock::new(MemoryWorldState::new()));

        // Create chain state
        let chain = Arc::new(RwLock::new(ChainState::new()));

        // Create mempool
        let mempool = Arc::new(RwLock::new(Mempool::new()));

        // Create consensus service if this is a validator
        let consensus = Arc::new(RwLock::new(config.validator_index.map(|index| {
            ConsensusService::new(
                config.network_magic,
                config.validators.clone(),
                Some(index),
                config.private_key.clone(),
                consensus_tx.clone(),
            )
        })));

        // Create state store if state service is enabled
        let full_state = config
            .state_service
            .as_ref()
            .map(|s| s.full_state)
            .unwrap_or(false);

        // Create state validator with proper verifier configuration
        let state_validator = config.state_service.as_ref().map(|settings| {
            debug!(
                target: "neo::runtime",
                path = %settings.path,
                full_state = settings.full_state,
                "initializing state root validator with network verification"
            );
            let validator_config = StateValidatorConfig {
                validate_on_receive: true,
                validate_after_execution: true,
                auto_resync: true,
                max_resync_blocks: 500,
            };
            Arc::new(StateRootValidator::new(
                validator_config,
                Arc::new(config.protocol_settings.clone()),
                settings.clone(),
            ))
        });

        // Get state store from validator if available, otherwise create standalone
        let state_store = state_validator.as_ref().map(|v| v.state_store().clone());

        // Create state trie manager for MPT-based state root calculation
        let state_trie = Arc::new(RwLock::new(StateTrieManager::new(full_state)));
        debug!(
            target: "neo::runtime",
            full_state,
            "initialized state trie manager for MPT state root calculation"
        );

        // Create block executor for transaction processing
        let block_executor = Arc::new(BlockExecutorImpl::new(config.protocol_settings.clone()));
        debug!(
            target: "neo::runtime",
            registered_contracts = block_executor.registered_contract_count(),
            "initialized block executor"
        );

        Self {
            config,
            state,
            chain,
            mempool,
            consensus,
            state_store,
            state_trie,
            state_validator,
            block_executor,
            chain_tx,
            consensus_tx,
            consensus_rx: Some(consensus_rx),
            p2p_tx,
            p2p_rx: Some(p2p_rx),
            event_tx,
            shutdown_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            p2p_broadcast_tx: None,
        }
    }

    /// Sets the P2P broadcast sender for consensus message broadcasting
    pub fn set_p2p_broadcast_sender(
        &mut self,
        tx: broadcast::Sender<crate::p2p_service::BroadcastMessage>,
    ) {
        self.p2p_broadcast_tx = Some(tx);
    }

    /// Returns the current block height
    pub async fn height(&self) -> u32 {
        self.chain.read().await.height()
    }

    /// Returns the number of transactions in the mempool
    pub async fn mempool_size(&self) -> usize {
        self.mempool.read().await.len()
    }

    /// Returns true if state root calculation is enabled
    pub fn state_root_enabled(&self) -> bool {
        self.state_store.is_some()
    }

    /// Returns the current local state root index
    pub fn local_state_root_index(&self) -> Option<u32> {
        self.state_store.as_ref().and_then(|s| s.local_root_index())
    }

    /// Returns the current validated state root index
    pub fn validated_state_root_index(&self) -> Option<u32> {
        self.state_store
            .as_ref()
            .and_then(|s| s.validated_root_index())
    }

    /// Returns the current local state root hash
    pub fn local_state_root_hash(&self) -> Option<neo_core::UInt256> {
        self.state_store
            .as_ref()
            .and_then(|s| s.current_local_root_hash())
    }

    /// Returns true if the node is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Subscribes to runtime events
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.event_tx.subscribe()
    }

    /// Returns a sender for chain events
    pub fn chain_event_sender(&self) -> broadcast::Sender<ChainEvent> {
        self.chain_tx.clone()
    }

    /// Returns a sender for consensus events
    pub fn consensus_event_sender(&self) -> mpsc::Sender<ConsensusEvent> {
        self.consensus_tx.clone()
    }

    /// Returns a sender for P2P events
    pub fn p2p_event_sender(&self) -> mpsc::Sender<P2PEvent> {
        self.p2p_tx.clone()
    }

    /// Initializes the genesis block if the chain is empty.
    async fn initialize_genesis(&self) -> anyhow::Result<()> {
        // Check if chain is already initialized
        {
            let chain_guard = self.chain.read().await;
            if chain_guard.is_initialized() {
                debug!(
                    target: "neo::runtime",
                    height = chain_guard.height(),
                    "chain already initialized, skipping genesis"
                );
                return Ok(());
            }
        }

        info!(
            target: "neo::runtime",
            network_magic = format!("0x{:08x}", self.config.network_magic),
            "initializing genesis block"
        );

        // Create genesis block from protocol settings
        let mut genesis_block = create_genesis_block(&self.config.protocol_settings);
        let genesis_hash = Block::hash(&mut genesis_block);
        let height = genesis_block.index();

        info!(
            target: "neo::runtime",
            hash = %genesis_hash,
            timestamp = genesis_block.header.timestamp(),
            next_consensus = %genesis_block.header.next_consensus(),
            "genesis block created"
        );

        // Execute genesis block to initialize native contracts
        let snapshot = Arc::new(DataCache::new(false));
        let execution_result = self.block_executor.execute_block(&genesis_block, snapshot);

        let state_changes = match execution_result {
            Ok(result) => {
                info!(
                    target: "neo::runtime",
                    gas_consumed = result.total_gas_consumed,
                    storage_changes = result.state_changes.storage.len(),
                    "genesis block executed successfully"
                );
                result.state_changes
            }
            Err(e) => {
                warn!(
                    target: "neo::runtime",
                    error = %e,
                    "genesis block execution failed, using empty state"
                );
                neo_state::StateChanges::new()
            }
        };

        // Calculate MPT state root from genesis execution
        let calculated_root = {
            let mut trie = self.state_trie.write().await;
            match trie.apply_changes(height, &state_changes) {
                Ok(root) => root,
                Err(e) => {
                    warn!(
                        target: "neo::runtime",
                        error = %e,
                        "failed to calculate genesis state root"
                    );
                    genesis_hash
                }
            }
        };

        info!(
            target: "neo::runtime",
            state_root = %calculated_root,
            "genesis state root calculated"
        );

        // Commit state changes to WorldState
        {
            let mut world_state = self.state.write().await;
            if let Err(e) = world_state.commit(state_changes) {
                warn!(
                    target: "neo::runtime",
                    error = %e,
                    "failed to commit genesis state changes"
                );
            }
        }

        // Initialize chain with genesis block
        let entry = BlockIndexEntry {
            hash: genesis_hash,
            height,
            prev_hash: *genesis_block.header.prev_hash(),
            header: genesis_block.header.to_array().unwrap_or_default(),
            timestamp: genesis_block.header.timestamp(),
            tx_count: genesis_block.transactions.len(),
            size: 0, // Genesis block size not critical
            cumulative_difficulty: 1,
            on_main_chain: true,
        };

        {
            let chain_guard = self.chain.write().await;
            if let Err(e) = chain_guard.init_genesis(entry) {
                anyhow::bail!("failed to initialize chain with genesis: {}", e);
            }
        }

        // Store genesis state root if state service is enabled
        if let Some(ref store) = self.state_store {
            let state_root = StateRoot::new_current(height, calculated_root);
            let snapshot = store.get_snapshot();
            if let Err(e) = snapshot.add_local_state_root(&state_root) {
                warn!(
                    target: "neo::runtime",
                    error = %e,
                    "failed to store genesis state root"
                );
            }
        }

        // Emit genesis initialized event
        let _ = self.chain_tx.send(ChainEvent::GenesisInitialized {
            hash: genesis_hash,
        });

        info!(
            target: "neo::runtime",
            hash = %genesis_hash,
            state_root = %calculated_root,
            "genesis block initialization complete"
        );

        Ok(())
    }

    /// Starts the node runtime
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running() {
            anyhow::bail!("Runtime is already running");
        }

        info!(
            target: "neo::runtime",
            network_magic = format!("0x{:08x}", self.config.network_magic),
            validator = self.config.validator_index.is_some(),
            "starting node runtime"
        );

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // Initialize genesis block if chain is empty
        self.initialize_genesis().await?;

        // Emit started event
        let _ = self.event_tx.send(RuntimeEvent::Started);

        // Take ownership of receivers
        let consensus_rx = self.consensus_rx.take();
        let p2p_rx = self.p2p_rx.take();

        // Spawn chain event processor
        {
            let mut chain_rx = self.chain_tx.subscribe();
            let event_tx = self.event_tx.clone();
            let state_trie = self.state_trie.clone();
            let state_store = self.state_store.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                handlers::process_chain_events(
                    &mut chain_rx,
                    event_tx,
                    state_trie,
                    state_store,
                    &mut shutdown_rx,
                )
                .await;
            });
        }

        // Spawn consensus event processor
        if let Some(rx) = consensus_rx {
            let event_tx = self.event_tx.clone();
            let mempool = self.mempool.clone();
            let consensus = self.consensus.clone();
            let p2p_broadcast_tx = self.p2p_broadcast_tx.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                handlers::process_consensus_events(
                    rx,
                    event_tx,
                    mempool,
                    consensus,
                    p2p_broadcast_tx,
                    &mut shutdown_rx,
                )
                .await;
            });
        }

        // Spawn P2P event processor
        if let Some(rx) = p2p_rx {
            let event_tx = self.event_tx.clone();
            let chain_tx = self.chain_tx.clone();
            let chain = self.chain.clone();
            let state = self.state.clone();
            let state_store = self.state_store.clone();
            let state_trie = self.state_trie.clone();
            let state_validator = self.state_validator.clone();
            let block_executor = self.block_executor.clone();
            let consensus = self.consensus.clone();
            let network_magic = self.config.network_magic;
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                handlers::process_p2p_events(
                    rx,
                    event_tx,
                    network_magic,
                    chain_tx,
                    chain,
                    state,
                    state_store,
                    state_trie,
                    state_validator,
                    block_executor,
                    consensus,
                    &mut shutdown_rx,
                )
                .await;
            });
        }

        info!(target: "neo::runtime", "node runtime started");
        Ok(())
    }

    /// Stops the node runtime
    pub async fn stop(&self) -> anyhow::Result<()> {
        if !self.is_running() {
            return Ok(());
        }

        info!(target: "neo::runtime", "stopping node runtime");

        // Emit stopping event
        let _ = self.event_tx.send(RuntimeEvent::Stopping);

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        info!(target: "neo::runtime", "node runtime stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_consensus::ValidatorInfo;

    #[tokio::test]
    async fn test_runtime_creation() {
        let config = RuntimeConfig::default();
        let runtime = NodeRuntime::new(config);

        assert!(!runtime.is_running());
        assert_eq!(runtime.height().await, 0);
    }

    #[tokio::test]
    async fn test_runtime_start_stop() {
        let config = RuntimeConfig::default();
        let mut runtime = NodeRuntime::new(config);

        runtime.start().await.unwrap();
        assert!(runtime.is_running());

        runtime.stop().await.unwrap();
        assert!(!runtime.is_running());
    }

    #[tokio::test]
    async fn test_runtime_event_subscription() {
        let config = RuntimeConfig::default();
        let mut runtime = NodeRuntime::new(config);
        let mut rx = runtime.subscribe();

        runtime.start().await.unwrap();

        // Should receive Started event
        let event = rx.recv().await.unwrap();
        assert!(matches!(event, RuntimeEvent::Started));

        runtime.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_validator_runtime() {
        use neo_core::UInt160;
        use neo_core::{ECCurve, ECPoint};

        let validators: Vec<ValidatorInfo> = (0..7)
            .map(|i| ValidatorInfo {
                index: i,
                public_key: ECPoint::infinity(ECCurve::Secp256r1),
                script_hash: UInt160::zero(),
            })
            .collect();

        let config = RuntimeConfig {
            validator_index: Some(0),
            validators,
            private_key: vec![0u8; 32],
            ..Default::default()
        };
        let runtime = NodeRuntime::new(config);

        assert!(runtime.consensus.read().await.is_some());
    }

    #[tokio::test]
    async fn test_state_trie_initialization() {
        let config = RuntimeConfig::default();
        let runtime = NodeRuntime::new(config);

        // State trie should be initialized but empty
        let trie = runtime.state_trie.read().await;
        assert!(trie.root_hash().is_none());
        assert_eq!(trie.current_index(), 0);
    }

    #[tokio::test]
    async fn test_state_trie_with_state_service() {
        use neo_core::state_service::state_store::StateServiceSettings;

        let config = RuntimeConfig {
            state_service: Some(StateServiceSettings {
                path: "/tmp/test".to_string(),
                full_state: true,
            }),
            ..Default::default()
        };
        let runtime = NodeRuntime::new(config);

        // State store should be enabled
        assert!(runtime.state_root_enabled());

        // State trie should be initialized
        let trie = runtime.state_trie.read().await;
        assert!(trie.root_hash().is_none());
    }

    #[tokio::test]
    async fn test_state_trie_apply_changes() {
        use neo_state::primitives::UInt160;
        use neo_state::{StateChanges, StorageItem, StorageKey};

        let config = RuntimeConfig::default();
        let runtime = NodeRuntime::new(config);

        // Apply some state changes
        let mut changes = StateChanges::new();
        let key = StorageKey::new(UInt160::default(), vec![0x01, 0x02]);
        let item = StorageItem::new(vec![0x03, 0x04, 0x05]);
        changes.storage.insert(key, Some(item));

        // Apply changes to trie
        {
            let mut trie = runtime.state_trie.write().await;
            let root = trie.apply_changes(1, &changes).unwrap();
            assert_ne!(root, neo_state::primitives::UInt256::zero());
        }

        // Verify state was updated
        let trie = runtime.state_trie.read().await;
        assert!(trie.root_hash().is_some());
        assert_eq!(trie.current_index(), 1);
    }

    #[tokio::test]
    async fn test_state_trie_incremental_blocks() {
        use neo_state::primitives::UInt160;
        use neo_state::{StateChanges, StorageItem, StorageKey};

        let config = RuntimeConfig::default();
        let runtime = NodeRuntime::new(config);

        let mut roots = Vec::new();

        // Simulate processing multiple blocks
        for block_index in 1u32..=5 {
            let mut changes = StateChanges::new();
            let key = StorageKey::new(UInt160::default(), block_index.to_le_bytes().to_vec());
            let item = StorageItem::new(vec![block_index as u8; 32]);
            changes.storage.insert(key, Some(item));

            let mut trie = runtime.state_trie.write().await;
            let root = trie.apply_changes(block_index, &changes).unwrap();
            roots.push(root);
        }

        // Each block should produce a different root
        for i in 1..roots.len() {
            assert_ne!(
                roots[i],
                roots[i - 1],
                "Block {} should have different root",
                i + 1
            );
        }

        // Final state should reflect all blocks
        let trie = runtime.state_trie.read().await;
        assert_eq!(trie.current_index(), 5);
        assert_eq!(trie.root_hash(), Some(roots[4]));
    }
}

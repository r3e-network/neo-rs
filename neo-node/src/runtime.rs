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

use crate::executor::BlockExecutorImpl;
use crate::state_validator::{StateRootValidator, StateValidatorConfig, ValidationResult};
use neo_chain::{BlockIndexEntry, ChainEvent, ChainState};
use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_core::neo_io::{MemoryReader, Serializable};
use crate::genesis::create_genesis_block;
use neo_core::network::p2p::payloads::Block;
use neo_core::persistence::data_cache::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::state_service::{state_store::StateServiceSettings, StateRoot, StateStore};
use neo_core::IVerifiable;
use neo_mempool::{Mempool, MempoolConfig};
use neo_p2p::{P2PConfig, P2PEvent};
use neo_state::{MemoryWorldState, StateTrieManager, WorldState};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Channel buffer sizes
const CHAIN_CHANNEL_SIZE: usize = 256;
const CONSENSUS_CHANNEL_SIZE: usize = 128;
const P2P_CHANNEL_SIZE: usize = 512;
const SHUTDOWN_CHANNEL_SIZE: usize = 8;

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Network magic number
    pub network_magic: u32,
    /// Protocol version
    pub protocol_version: u32,
    /// Validator index (None if not a validator)
    pub validator_index: Option<u8>,
    /// Validator list
    pub validators: Vec<ValidatorInfo>,
    /// Private key for signing (empty if not a validator)
    pub private_key: Vec<u8>,
    /// P2P configuration
    pub p2p: P2PConfig,
    /// Mempool configuration
    pub mempool: MempoolConfig,
    /// State service settings (None to disable state root calculation)
    pub state_service: Option<StateServiceSettings>,
    /// Protocol settings for block execution
    pub protocol_settings: ProtocolSettings,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            network_magic: 0x4F454E, // "NEO"
            protocol_version: 0,
            validator_index: None,
            validators: Vec::new(),
            private_key: Vec::new(),
            p2p: P2PConfig::default(),
            mempool: MempoolConfig::default(),
            state_service: None, // Disabled by default
            protocol_settings: ProtocolSettings::default(),
        }
    }
}

/// Events emitted by the runtime
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// Node started
    Started,
    /// Node stopping
    Stopping,
    /// New block applied
    BlockApplied { height: u32, hash: [u8; 32] },
    /// New transaction added to mempool
    TransactionAdded { hash: [u8; 32] },
    /// Peer connected
    PeerConnected { address: String },
    /// Peer disconnected
    PeerDisconnected { address: String },
    /// Consensus state changed
    ConsensusStateChanged { view: u8, block_index: u32 },
}

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
    pub fn set_p2p_broadcast_sender(&mut self, tx: broadcast::Sender<crate::p2p_service::BroadcastMessage>) {
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
    ///
    /// This method:
    /// 1. Checks if the chain is already initialized
    /// 2. Creates the genesis block from protocol settings
    /// 3. Executes the genesis block to initialize native contracts
    /// 4. Commits the initial state to WorldState
    /// 5. Calculates and stores the initial state root
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
                Self::process_chain_events(
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
                Self::process_consensus_events(rx, event_tx, mempool, consensus, p2p_broadcast_tx, &mut shutdown_rx).await;
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
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                Self::process_p2p_events(
                    rx,
                    event_tx,
                    chain_tx,
                    chain,
                    state,
                    state_store,
                    state_trie,
                    state_validator,
                    block_executor,
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

    /// Processes chain events
    async fn process_chain_events(
        rx: &mut broadcast::Receiver<ChainEvent>,
        event_tx: broadcast::Sender<RuntimeEvent>,
        state_trie: Arc<RwLock<StateTrieManager>>,
        state_store: Option<Arc<StateStore>>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            match event {
                                ChainEvent::BlockAdded { hash, height, on_main_chain } => {
                                    if on_main_chain {
                                        info!(
                                            target: "neo::runtime",
                                            height,
                                            hash = %hash,
                                            "block added to main chain"
                                        );
                                        let hash_bytes: [u8; 32] = hash.to_bytes().try_into().unwrap_or([0u8; 32]);
                                        let _ = event_tx.send(RuntimeEvent::BlockApplied {
                                            height,
                                            hash: hash_bytes,
                                        });
                                    }
                                }
                                ChainEvent::TipChanged { new_hash, new_height, prev_hash } => {
                                    info!(
                                        target: "neo::runtime",
                                        new_height,
                                        new_hash = %new_hash,
                                        prev_hash = %prev_hash,
                                        "chain tip changed"
                                    );
                                }
                                ChainEvent::Reorganization { fork_point, disconnected, connected } => {
                                    warn!(
                                        target: "neo::runtime",
                                        fork_point = %fork_point,
                                        disconnected_count = disconnected.len(),
                                        connected_count = connected.len(),
                                        "chain reorganization detected, initiating state rollback"
                                    );

                                    // Get the state root at the fork point for rollback
                                    if let Some(ref store) = state_store {
                                        // Find the block height at fork point
                                        // Estimate reorg depth based on disconnected blocks
                                        let rollback_height = {
                                            let trie = state_trie.read().await;
                                            let current = trie.current_index();
                                            current.saturating_sub(disconnected.len() as u32)
                                        };

                                        // Try to get the state root at the fork point
                                        let snapshot = store.get_snapshot();
                                        if let Some(fork_root) = snapshot.get_state_root(rollback_height) {
                                            info!(
                                                target: "neo::runtime",
                                                rollback_height,
                                                fork_root = %fork_root.root_hash,
                                                "rolling back state trie to fork point"
                                            );

                                            // Reset state trie to fork point
                                            let mut trie = state_trie.write().await;
                                            trie.reset_to_root(fork_root.root_hash, rollback_height);

                                            info!(
                                                target: "neo::runtime",
                                                new_index = trie.current_index(),
                                                "state rollback complete, ready to apply new chain"
                                            );
                                        } else {
                                            warn!(
                                                target: "neo::runtime",
                                                rollback_height,
                                                "state root not found for rollback height, full resync may be needed"
                                            );
                                        }
                                    } else {
                                        debug!(
                                            target: "neo::runtime",
                                            "state store not enabled, skipping state rollback"
                                        );
                                    }

                                    // Note: Connected blocks will be re-executed via P2P BlockReceived events
                                    info!(
                                        target: "neo::runtime",
                                        connected_count = connected.len(),
                                        "awaiting re-execution of {} connected blocks",
                                        connected.len()
                                    );
                                }
                                ChainEvent::GenesisInitialized { hash } => {
                                    info!(
                                        target: "neo::runtime",
                                        hash = %hash,
                                        "genesis block initialized"
                                    );
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(target: "neo::runtime", lagged = n, "chain event receiver lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!(target: "neo::runtime", "chain event channel closed");
                            break;
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!(target: "neo::runtime", "chain event processor shutting down");
                    break;
                }
            }
        }
    }

    /// Processes consensus events
    async fn process_consensus_events(
        mut rx: mpsc::Receiver<ConsensusEvent>,
        event_tx: broadcast::Sender<RuntimeEvent>,
        mempool: Arc<RwLock<Mempool>>,
        consensus: Arc<RwLock<Option<ConsensusService>>>,
        p2p_broadcast_tx: Option<broadcast::Sender<crate::p2p_service::BroadcastMessage>>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        ConsensusEvent::ViewChanged { block_index, old_view, new_view } => {
                            info!(
                                target: "neo::runtime",
                                block_index,
                                old_view,
                                new_view,
                                "consensus view changed"
                            );
                            let _ = event_tx.send(RuntimeEvent::ConsensusStateChanged {
                                view: new_view,
                                block_index,
                            });
                        }
                        ConsensusEvent::BlockCommitted { block_index, block_hash, block_data } => {
                            info!(
                                target: "neo::runtime",
                                block_index,
                                block_hash = %block_hash,
                                signature_count = block_data.signatures.len(),
                                required_sigs = block_data.required_signatures,
                                validators = block_data.validator_pubkeys.len(),
                                tx_count = block_data.transaction_hashes.len(),
                                "block committed by consensus - ready for assembly"
                            );
                            // Block assembly and persistence is handled by ValidatorService.handle_consensus_event()
                            // which calls assemble_block() to construct the complete Block with multi-sig witness,
                            // then persists via ChainState. This handler only logs for non-validator nodes.
                        }
                        ConsensusEvent::BroadcastMessage(payload) => {
                            info!(
                                target: "neo::runtime",
                                block_index = payload.block_index,
                                validator_index = payload.validator_index,
                                view_number = payload.view_number,
                                msg_type = ?payload.message_type,
                                data_len = payload.data.len(),
                                "broadcasting consensus message to peers"
                            );
                            // Broadcast consensus message to all peers via P2P service
                            if let Some(ref tx) = p2p_broadcast_tx {
                                let broadcast_msg = crate::p2p_service::BroadcastMessage {
                                    message: payload.data.clone(),
                                    category: "dBFT".to_string(),
                                };
                                if let Err(e) = tx.send(broadcast_msg) {
                                    warn!(
                                        target: "neo::runtime",
                                        error = %e,
                                        "failed to broadcast consensus message"
                                    );
                                } else {
                                    debug!(
                                        target: "neo::runtime",
                                        "consensus message sent to P2P broadcast channel"
                                    );
                                }
                            } else {
                                debug!(
                                    target: "neo::runtime",
                                    "P2P broadcast channel not configured"
                                );
                            }
                        }
                        ConsensusEvent::RequestTransactions { block_index, max_count } => {
                            info!(
                                target: "neo::runtime",
                                block_index,
                                max_count,
                                "consensus requesting transactions"
                            );
                            // Get top transactions from mempool by fee
                            let mempool_guard = mempool.read().await;
                            let top_txs = mempool_guard.get_top(max_count as usize);
                            drop(mempool_guard);

                            // Extract transaction hashes
                            let tx_hashes: Vec<neo_core::UInt256> = top_txs
                                .iter()
                                .map(|entry| entry.hash)
                                .collect();
                            let tx_count = tx_hashes.len();

                            if tx_count > 0 {
                                info!(
                                    target: "neo::runtime",
                                    block_index,
                                    tx_count,
                                    "retrieved transactions from mempool for consensus"
                                );

                                // Send transaction hashes back to consensus service
                                let mut consensus_guard = consensus.write().await;
                                if let Some(ref mut consensus_service) = *consensus_guard {
                                    if let Err(e) = consensus_service.on_transactions_received(tx_hashes) {
                                        warn!(
                                            target: "neo::runtime",
                                            error = %e,
                                            "failed to send transactions to consensus"
                                        );
                                    } else {
                                        debug!(
                                            target: "neo::runtime",
                                            tx_count,
                                            "transactions sent to consensus service"
                                        );
                                    }
                                }
                            } else {
                                debug!(
                                    target: "neo::runtime",
                                    block_index,
                                    "no transactions available in mempool"
                                );
                            }
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!(target: "neo::runtime", "consensus event processor shutting down");
                    break;
                }
            }
        }
    }

    /// Processes P2P events
    async fn process_p2p_events(
        mut rx: mpsc::Receiver<P2PEvent>,
        event_tx: broadcast::Sender<RuntimeEvent>,
        chain_tx: broadcast::Sender<ChainEvent>,
        chain: Arc<RwLock<ChainState>>,
        state: Arc<RwLock<MemoryWorldState>>,
        state_store: Option<Arc<StateStore>>,
        state_trie: Arc<RwLock<StateTrieManager>>,
        state_validator: Option<Arc<StateRootValidator>>,
        block_executor: Arc<BlockExecutorImpl>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        loop {
            tokio::select! {
                Some(event) = rx.recv() => {
                    match event {
                        P2PEvent::PeerConnected(info) => {
                            info!(
                                target: "neo::runtime",
                                address = %info.address,
                                "peer connected"
                            );
                            let _ = event_tx.send(RuntimeEvent::PeerConnected {
                                address: info.address.to_string(),
                            });
                        }
                        P2PEvent::PeerDisconnected(addr) => {
                            info!(
                                target: "neo::runtime",
                                address = %addr,
                                "peer disconnected"
                            );
                            let _ = event_tx.send(RuntimeEvent::PeerDisconnected {
                                address: addr.to_string(),
                            });
                        }
                        P2PEvent::BlockReceived { hash, data, from } => {
                            // Deserialize block from data
                            if data.is_empty() {
                                warn!(target: "neo::runtime", hash = %hash, "received empty block data");
                                continue;
                            }

                            let mut reader = MemoryReader::new(&data);
                            match Block::deserialize(&mut reader) {
                                Ok(block) => {
                                    let block_hash = match block.hash() {
                                        Ok(hash) => hash,
                                        Err(e) => {
                                            warn!(
                                                target: "neo::runtime",
                                                error = %e,
                                                "failed to calculate block hash"
                                            );
                                            continue;
                                        }
                                    };
                                    let height = block.index();
                                    let tx_count = block.transactions.len();

                                    info!(
                                        target: "neo::runtime",
                                        height,
                                        hash = %block_hash,
                                        tx_count,
                                        from = %from,
                                        "block received and deserialized"
                                    );

                                    // Create block index entry
                                    let entry = BlockIndexEntry {
                                        hash: block_hash,
                                        height,
                                        prev_hash: *block.header.prev_hash(),
                                        timestamp: block.header.timestamp(),
                                        tx_count,
                                        size: data.len(),
                                        cumulative_difficulty: height as u64 + 1,
                                        on_main_chain: false,
                                    };

                                    // Try to add block to chain state
                                    let chain_guard = chain.write().await;

                                    // Initialize chain with genesis block (height 0)
                                    if !chain_guard.is_initialized() {
                                        if height == 0 {
                                            // This is the actual genesis block - use init_genesis
                                            if let Err(e) = chain_guard.init_genesis(entry.clone()) {
                                                warn!(
                                                    target: "neo::runtime",
                                                    error = %e,
                                                    "failed to initialize chain with genesis block"
                                                );
                                            } else {
                                                info!(
                                                    target: "neo::runtime",
                                                    hash = %block_hash,
                                                    "chain initialized with genesis block"
                                                );

                                                // Emit genesis initialized event
                                                let _ = chain_tx.send(ChainEvent::GenesisInitialized {
                                                    hash: block_hash,
                                                });
                                            }
                                            // Genesis block is already added by init_genesis, skip add_block
                                            drop(chain_guard);
                                            continue;
                                        } else {
                                            // Received non-genesis block but chain not initialized
                                            // Request genesis block first
                                            warn!(
                                                target: "neo::runtime",
                                                height,
                                                "received block but chain not initialized, waiting for genesis"
                                            );
                                            drop(chain_guard);
                                            continue;
                                        }
                                    }

                                    match chain_guard.add_block(entry) {
                                        Ok(is_new_tip) => {
                                            if is_new_tip {
                                                info!(
                                                    target: "neo::runtime",
                                                    height,
                                                    hash = %block_hash,
                                                    "new chain tip"
                                                );

                                                // Emit chain event
                                                let _ = chain_tx.send(ChainEvent::BlockAdded {
                                                    hash: block_hash,
                                                    height,
                                                    on_main_chain: true,
                                                });

                                                // Emit runtime event
                                                let hash_bytes: [u8; 32] = block_hash.to_bytes().try_into().unwrap_or([0u8; 32]);
                                                let _ = event_tx.send(RuntimeEvent::BlockApplied {
                                                    height,
                                                    hash: hash_bytes,
                                                });

                                                // Execute block via BlockExecutorImpl
                                                // This performs full transaction execution:
                                                // 1. OnPersist - Native contract state updates
                                                // 2. Application - Execute each transaction
                                                // 3. PostPersist - Native contract cleanup
                                                let snapshot = Arc::new(DataCache::new(false));
                                                let execution_result = block_executor.execute_block(&block, snapshot);

                                                let state_changes = match execution_result {
                                                    Ok(result) => {
                                                        info!(
                                                            target: "neo::runtime",
                                                            height,
                                                            successful_tx = result.successful_tx_count,
                                                            failed_tx = result.failed_tx_count,
                                                            total_gas = result.total_gas_consumed,
                                                            storage_changes = result.state_changes.storage.len(),
                                                            "block executed successfully"
                                                        );
                                                        result.state_changes
                                                    }
                                                    Err(e) => {
                                                        warn!(
                                                            target: "neo::runtime",
                                                            height,
                                                            error = %e,
                                                            "block execution failed, using empty state changes"
                                                        );
                                                        neo_state::StateChanges::new()
                                                    }
                                                };

                                                // Calculate MPT state root from execution state changes
                                                let calculated_root = {
                                                    let mut trie = state_trie.write().await;
                                                    match trie.apply_changes(height, &state_changes) {
                                                        Ok(root) => root,
                                                        Err(e) => {
                                                            warn!(
                                                                target: "neo::runtime",
                                                                height,
                                                                error = %e,
                                                                "failed to calculate MPT state root, using block hash"
                                                            );
                                                            block_hash
                                                        }
                                                    }
                                                };

                                                info!(
                                                    target: "neo::runtime",
                                                    height,
                                                    calculated_root = %calculated_root,
                                                    block_hash = %block_hash,
                                                    storage_changes = state_changes.storage.len(),
                                                    "MPT state root calculated from block execution"
                                                );

                                                // Commit state changes to WorldState for persistence
                                                {
                                                    let mut world_state = state.write().await;
                                                    if let Err(e) = world_state.commit(state_changes.clone()) {
                                                        warn!(
                                                            target: "neo::runtime",
                                                            height,
                                                            error = %e,
                                                            "failed to commit state changes to WorldState"
                                                        );
                                                    } else {
                                                        debug!(
                                                            target: "neo::runtime",
                                                            height,
                                                            "state changes committed to WorldState"
                                                        );
                                                    }
                                                }

                                                // Update state store if enabled
                                                if let Some(ref store) = state_store {
                                                    let state_root = StateRoot::new_current(height, calculated_root);
                                                    let snapshot = store.get_snapshot();

                                                    match snapshot.add_local_state_root(&state_root) {
                                                        Ok(()) => {
                                                            info!(
                                                                target: "neo::runtime",
                                                                height,
                                                                root_hash = %calculated_root,
                                                                "local state root added to store"
                                                            );
                                                        }
                                                        Err(e) => {
                                                            warn!(
                                                                target: "neo::runtime",
                                                                height,
                                                                error = %e,
                                                                "failed to add local state root"
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            debug!(
                                                target: "neo::runtime",
                                                height,
                                                hash = %block_hash,
                                                error = %e,
                                                "failed to add block to chain"
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        target: "neo::runtime",
                                        hash = %hash,
                                        error = %e,
                                        "failed to deserialize block"
                                    );
                                }
                            }
                        }
                        P2PEvent::TransactionReceived { hash, from, .. } => {
                            info!(
                                target: "neo::runtime",
                                hash = %hash,
                                from = %from,
                                "transaction received from peer"
                            );
                        }
                        P2PEvent::HeadersReceived { headers, from } => {
                            info!(
                                target: "neo::runtime",
                                count = headers.len(),
                                from = %from,
                                "headers received from peer"
                            );
                        }
                        P2PEvent::InventoryReceived { inv_type, hashes, from } => {
                            info!(
                                target: "neo::runtime",
                                inv_type = ?inv_type,
                                count = hashes.len(),
                                from = %from,
                                "inventory received from peer"
                            );
                        }
                        P2PEvent::ConsensusReceived { from, .. } => {
                            info!(
                                target: "neo::runtime",
                                from = %from,
                                "consensus message received from peer"
                            );
                        }
                        P2PEvent::StateRootReceived { data, from } => {
                            // Deserialize and validate state root from network
                            let mut reader = MemoryReader::new(&data);
                            match StateRoot::deserialize(&mut reader) {
                                Ok(state_root) => {
                                    let index = state_root.index;
                                    let network_root = state_root.root_hash;

                                    info!(
                                        target: "neo::runtime",
                                        index,
                                        network_root = %network_root,
                                        from = %from,
                                        "state root received from network"
                                    );

                                    // Get local state root for comparison
                                    let local_root = state_trie.read().await.root_hash();
                                    let local_index = state_trie.read().await.current_index();

                                    // Use StateRootValidator for comprehensive validation
                                    if let Some(ref validator) = state_validator {
                                        // Validate with signature verification and auto-resync
                                        let result = validator.validate_network_state_root(
                                            state_root.clone(),
                                            local_root,
                                            local_index,
                                        ).await;

                                        match result {
                                            ValidationResult::Valid { index, root_hash } => {
                                                info!(
                                                    target: "neo::runtime",
                                                    index,
                                                    root_hash = %root_hash,
                                                    "✅ STATE ROOT VALIDATED: signature verified, matches local"
                                                );
                                            }
                                            ValidationResult::Mismatch { index, local_root, network_root } => {
                                                error!(
                                                    target: "neo::runtime",
                                                    index,
                                                    local_root = %local_root,
                                                    network_root = %network_root,
                                                    "❌ STATE ROOT MISMATCH: auto-resync triggered"
                                                );
                                            }
                                            ValidationResult::InvalidSignature { index } => {
                                                warn!(
                                                    target: "neo::runtime",
                                                    index,
                                                    from = %from,
                                                    "⚠️ STATE ROOT REJECTED: invalid signature"
                                                );
                                            }
                                            ValidationResult::MissingWitness { index } => {
                                                debug!(
                                                    target: "neo::runtime",
                                                    index,
                                                    "state root missing witness, skipping validation"
                                                );
                                            }
                                            ValidationResult::LocalNotAvailable { index } => {
                                                debug!(
                                                    target: "neo::runtime",
                                                    index,
                                                    "local state root not available for comparison"
                                                );
                                            }
                                            ValidationResult::IndexMismatch { local_index, network_index } => {
                                                debug!(
                                                    target: "neo::runtime",
                                                    local_index,
                                                    network_index,
                                                    "state root index mismatch, cannot compare"
                                                );
                                            }
                                        }
                                    } else {
                                        // Fallback: simple comparison without signature verification
                                        if let Some(local) = local_root {
                                            if local_index == index {
                                                if local == network_root {
                                                    info!(
                                                        target: "neo::runtime",
                                                        index,
                                                        root_hash = %local,
                                                        "✅ STATE ROOT MATCH: local matches network (no signature verification)"
                                                    );
                                                } else {
                                                    warn!(
                                                        target: "neo::runtime",
                                                        index,
                                                        local_root = %local,
                                                        network_root = %network_root,
                                                        "❌ STATE ROOT MISMATCH: local differs from network!"
                                                    );
                                                }
                                            }
                                        }

                                        // Store via state_store if available
                                        if let Some(ref store) = state_store {
                                            if store.on_new_state_root(state_root) {
                                                info!(
                                                    target: "neo::runtime",
                                                    index,
                                                    "validated state root accepted"
                                                );
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(
                                        target: "neo::runtime",
                                        from = %from,
                                        error = %e,
                                        "failed to deserialize state root"
                                    );
                                }
                            }
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!(target: "neo::runtime", "p2p event processor shutting down");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

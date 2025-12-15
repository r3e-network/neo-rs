//! Neo Node Runtime
//!
//! This module provides the main runtime for the Neo N3 node, integrating:
//! - World state management (neo-state)
//! - Chain state controller (neo-chain)
//! - Transaction mempool (neo-mempool)
//! - P2P networking (neo-p2p)
//! - dBFT consensus (neo-consensus)
//! - State root calculation (neo-core state_service)
//!
//! Communication between components uses pure tokio channels for best performance.

use neo_chain::{BlockIndexEntry, ChainEvent, ChainState};
use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_core::network::p2p::payloads::Block;
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::state_service::{StateRoot, StateStore, state_store::StateServiceSettings};
use neo_mempool::{Mempool, MempoolConfig};
use neo_p2p::{P2PConfig, P2PEvent};
use neo_core::UInt256;
use neo_state::MemoryWorldState;
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
    consensus: Option<ConsensusService>,
    /// State store for state root calculation (optional)
    state_store: Option<Arc<StateStore>>,
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
        let consensus = config.validator_index.map(|index| {
            ConsensusService::new(
                config.network_magic,
                config.validators.clone(),
                Some(index),
                config.private_key.clone(),
                consensus_tx.clone(),
            )
        });

        // Create state store if state service is enabled
        let state_store = config.state_service.as_ref().map(|settings| {
            debug!(
                target: "neo::runtime",
                path = %settings.path,
                full_state = settings.full_state,
                "initializing state store for state root calculation"
            );
            Arc::new(StateStore::new_in_memory()) // TODO: Use persistent backend
        });

        Self {
            config,
            state,
            chain,
            mempool,
            consensus,
            state_store,
            chain_tx,
            consensus_tx,
            consensus_rx: Some(consensus_rx),
            p2p_tx,
            p2p_rx: Some(p2p_rx),
            event_tx,
            shutdown_tx,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
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
        self.state_store.as_ref().and_then(|s| s.validated_root_index())
    }

    /// Returns the current local state root hash
    pub fn local_state_root_hash(&self) -> Option<neo_core::UInt256> {
        self.state_store.as_ref().and_then(|s| s.current_local_root_hash())
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

        // Emit started event
        let _ = self.event_tx.send(RuntimeEvent::Started);

        // Take ownership of receivers
        let consensus_rx = self.consensus_rx.take();
        let p2p_rx = self.p2p_rx.take();

        // Spawn chain event processor
        {
            let mut chain_rx = self.chain_tx.subscribe();
            let event_tx = self.event_tx.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                Self::process_chain_events(&mut chain_rx, event_tx, &mut shutdown_rx).await;
            });
        }

        // Spawn consensus event processor
        if let Some(rx) = consensus_rx {
            let event_tx = self.event_tx.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                Self::process_consensus_events(rx, event_tx, &mut shutdown_rx).await;
            });
        }

        // Spawn P2P event processor
        if let Some(rx) = p2p_rx {
            let event_tx = self.event_tx.clone();
            let chain_tx = self.chain_tx.clone();
            let chain = self.chain.clone();
            let state_store = self.state_store.clone();
            let mut shutdown_rx = self.shutdown_tx.subscribe();

            tokio::spawn(async move {
                Self::process_p2p_events(rx, event_tx, chain_tx, chain, state_store, &mut shutdown_rx).await;
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
                                        "chain reorganization"
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
                        ConsensusEvent::BlockCommitted { block_index, block_hash, signatures } => {
                            info!(
                                target: "neo::runtime",
                                block_index,
                                block_hash = %block_hash,
                                signature_count = signatures.len(),
                                "block committed by consensus"
                            );
                        }
                        ConsensusEvent::BroadcastMessage(payload) => {
                            info!(
                                target: "neo::runtime",
                                block_index = payload.block_index,
                                msg_type = ?payload.message_type,
                                "broadcasting consensus message"
                            );
                            // TODO: Send to P2P layer
                        }
                        ConsensusEvent::RequestTransactions { block_index, max_count } => {
                            info!(
                                target: "neo::runtime",
                                block_index,
                                max_count,
                                "consensus requesting transactions"
                            );
                            // TODO: Get transactions from mempool
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
        state_store: Option<Arc<StateStore>>,
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
                                Ok(mut block) => {
                                    let block_hash = block.hash();
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

                                    // Initialize chain with this block if not initialized
                                    // (for sync nodes joining mid-chain)
                                    if !chain_guard.is_initialized() {
                                        // Use first received block as temporary genesis
                                        // In production, we'd fetch the real genesis
                                        let genesis_entry = BlockIndexEntry {
                                            hash: *block.header.prev_hash(),
                                            height: height.saturating_sub(1),
                                            prev_hash: UInt256::zero(),
                                            timestamp: block.header.timestamp().saturating_sub(15000),
                                            tx_count: 0,
                                            size: 0,
                                            cumulative_difficulty: height as u64,
                                            on_main_chain: true,
                                        };
                                        if let Err(e) = chain_guard.init_from_block(genesis_entry) {
                                            warn!(
                                                target: "neo::runtime",
                                                error = %e,
                                                "failed to initialize chain with synthetic genesis"
                                            );
                                        } else {
                                            info!(
                                                target: "neo::runtime",
                                                height = height.saturating_sub(1),
                                                "chain initialized with synthetic genesis for sync"
                                            );
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

                                                // Update state root if enabled
                                                if let Some(ref store) = state_store {
                                                    // Create local state root for this block.
                                                    // Note: In production, root_hash would be calculated
                                                    // from actual state changes via MPT trie. For now,
                                                    // we use block hash as a placeholder.
                                                    let state_root = StateRoot::new_current(height, block_hash);
                                                    let snapshot = store.get_snapshot();

                                                    match snapshot.add_local_state_root(&state_root) {
                                                        Ok(()) => {
                                                            info!(
                                                                target: "neo::runtime",
                                                                height,
                                                                root_hash = %block_hash,
                                                                "local state root added"
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
                                Ok(mut state_root) => {
                                    let index = state_root.index;
                                    let root_hash = state_root.root_hash;

                                    info!(
                                        target: "neo::runtime",
                                        index,
                                        root_hash = %root_hash,
                                        from = %from,
                                        "state root received from network"
                                    );

                                    // Validate and store if state service is enabled
                                    if let Some(ref store) = state_store {
                                        if store.on_new_state_root(state_root) {
                                            info!(
                                                target: "neo::runtime",
                                                index,
                                                "validated state root accepted"
                                            );
                                        } else {
                                            debug!(
                                                target: "neo::runtime",
                                                index,
                                                "state root rejected (missing witness or already validated)"
                                            );
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
        use neo_core::{ECCurve, ECPoint};
        use neo_core::UInt160;

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

        assert!(runtime.consensus.is_some());
    }
}

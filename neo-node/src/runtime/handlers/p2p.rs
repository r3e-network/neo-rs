//! P2P event handler for the runtime.

use crate::executor::BlockExecutorImpl;
use crate::runtime::events::RuntimeEvent;
use crate::state_validator::{StateRootValidator, ValidationResult};
use neo_chain::{BlockIndexEntry, ChainEvent, ChainState};
use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::Block;
use neo_core::persistence::data_cache::DataCache;
use neo_core::state_service::{StateRoot, StateStore};
use neo_core::IVerifiable;
use neo_p2p::P2PEvent;
use neo_state::{MemoryWorldState, StateTrieManager, WorldState};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Processes P2P events from the network layer.
#[allow(clippy::too_many_arguments)]
pub async fn process_p2p_events(
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
                handle_p2p_event(
                    event,
                    &event_tx,
                    &chain_tx,
                    &chain,
                    &state,
                    &state_store,
                    &state_trie,
                    &state_validator,
                    &block_executor,
                ).await;
            }
            _ = shutdown_rx.recv() => {
                info!(target: "neo::runtime", "p2p event processor shutting down");
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_p2p_event(
    event: P2PEvent,
    event_tx: &broadcast::Sender<RuntimeEvent>,
    chain_tx: &broadcast::Sender<ChainEvent>,
    chain: &Arc<RwLock<ChainState>>,
    state: &Arc<RwLock<MemoryWorldState>>,
    state_store: &Option<Arc<StateStore>>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_validator: &Option<Arc<StateRootValidator>>,
    block_executor: &Arc<BlockExecutorImpl>,
) {
    match event {
        P2PEvent::PeerConnected(info) => {
            handle_peer_connected(event_tx, &info.address.to_string());
        }
        P2PEvent::PeerDisconnected(addr) => {
            handle_peer_disconnected(event_tx, &addr.to_string());
        }
        P2PEvent::BlockReceived { hash, data, from } => {
            handle_block_received(
                hash,
                &data,
                &from.to_string(),
                event_tx,
                chain_tx,
                chain,
                state,
                state_store,
                state_trie,
                block_executor,
            )
            .await;
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
        P2PEvent::InventoryReceived {
            inv_type,
            hashes,
            from,
        } => {
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
            handle_state_root_received(&data, &from.to_string(), state_trie, state_validator, state_store).await;
        }
    }
}

fn handle_peer_connected(event_tx: &broadcast::Sender<RuntimeEvent>, address: &str) {
    info!(
        target: "neo::runtime",
        address,
        "peer connected"
    );
    let _ = event_tx.send(RuntimeEvent::PeerConnected {
        address: address.to_string(),
    });
}

fn handle_peer_disconnected(event_tx: &broadcast::Sender<RuntimeEvent>, address: &str) {
    info!(
        target: "neo::runtime",
        address,
        "peer disconnected"
    );
    let _ = event_tx.send(RuntimeEvent::PeerDisconnected {
        address: address.to_string(),
    });
}

#[allow(clippy::too_many_arguments)]
async fn handle_block_received(
    hash: neo_core::UInt256,
    data: &[u8],
    from: &str,
    event_tx: &broadcast::Sender<RuntimeEvent>,
    chain_tx: &broadcast::Sender<ChainEvent>,
    chain: &Arc<RwLock<ChainState>>,
    state: &Arc<RwLock<MemoryWorldState>>,
    state_store: &Option<Arc<StateStore>>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    block_executor: &Arc<BlockExecutorImpl>,
) {
    if data.is_empty() {
        warn!(target: "neo::runtime", hash = %hash, "received empty block data");
        return;
    }

    let mut reader = MemoryReader::new(data);
    match Block::deserialize(&mut reader) {
        Ok(block) => {
            let block_hash = match block.hash() {
                Ok(h) => h,
                Err(e) => {
                    warn!(
                        target: "neo::runtime",
                        error = %e,
                        "failed to calculate block hash"
                    );
                    return;
                }
            };
            let height = block.index();
            let tx_count = block.transactions.len();

            info!(
                target: "neo::runtime",
                height,
                hash = %block_hash,
                tx_count,
                from,
                "block received and deserialized"
            );

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

            let chain_guard = chain.write().await;

            // Initialize chain with genesis block (height 0)
            if !chain_guard.is_initialized() {
                if height == 0 {
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
                        let _ = chain_tx.send(ChainEvent::GenesisInitialized { hash: block_hash });
                    }
                    return;
                } else {
                    warn!(
                        target: "neo::runtime",
                        height,
                        "received block but chain not initialized, waiting for genesis"
                    );
                    return;
                }
            }

            match chain_guard.add_block(entry) {
                Ok(is_new_tip) => {
                    if is_new_tip {
                        process_new_tip(
                            &block,
                            block_hash,
                            height,
                            data.len(),
                            event_tx,
                            chain_tx,
                            state,
                            state_store,
                            state_trie,
                            block_executor,
                        )
                        .await;
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

#[allow(clippy::too_many_arguments)]
async fn process_new_tip(
    block: &Block,
    block_hash: neo_core::UInt256,
    height: u32,
    _data_len: usize,
    event_tx: &broadcast::Sender<RuntimeEvent>,
    chain_tx: &broadcast::Sender<ChainEvent>,
    state: &Arc<RwLock<MemoryWorldState>>,
    state_store: &Option<Arc<StateStore>>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    block_executor: &Arc<BlockExecutorImpl>,
) {
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
    let snapshot = Arc::new(DataCache::new(false));
    let execution_result = block_executor.execute_block(block, snapshot);

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
        if let Err(e) = world_state.commit(state_changes) {
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

async fn handle_state_root_received(
    data: &[u8],
    from: &str,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_validator: &Option<Arc<StateRootValidator>>,
    state_store: &Option<Arc<StateStore>>,
) {
    let mut reader = MemoryReader::new(data);
    match StateRoot::deserialize(&mut reader) {
        Ok(state_root) => {
            let index = state_root.index;
            let network_root = state_root.root_hash;

            info!(
                target: "neo::runtime",
                index,
                network_root = %network_root,
                from,
                "state root received from network"
            );

            let local_root = state_trie.read().await.root_hash();
            let local_index = state_trie.read().await.current_index();

            if let Some(ref validator) = state_validator {
                validate_with_validator(validator, state_root.clone(), local_root, local_index, from).await;
            } else {
                validate_without_validator(state_root, local_root, local_index, index, network_root, state_store);
            }
        }
        Err(e) => {
            warn!(
                target: "neo::runtime",
                from,
                error = %e,
                "failed to deserialize state root"
            );
        }
    }
}

async fn validate_with_validator(
    validator: &StateRootValidator,
    state_root: StateRoot,
    local_root: Option<neo_core::UInt256>,
    local_index: u32,
    from: &str,
) {
    let result = validator
        .validate_network_state_root(state_root, local_root, local_index)
        .await;

    match result {
        ValidationResult::Valid { index, root_hash } => {
            info!(
                target: "neo::runtime",
                index,
                root_hash = %root_hash,
                "STATE ROOT VALIDATED: signature verified, matches local"
            );
        }
        ValidationResult::Mismatch {
            index,
            local_root,
            network_root,
        } => {
            error!(
                target: "neo::runtime",
                index,
                local_root = %local_root,
                network_root = %network_root,
                "STATE ROOT MISMATCH: auto-resync triggered"
            );
        }
        ValidationResult::InvalidSignature { index } => {
            warn!(
                target: "neo::runtime",
                index,
                from,
                "STATE ROOT REJECTED: invalid signature"
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
        ValidationResult::IndexMismatch {
            local_index,
            network_index,
        } => {
            debug!(
                target: "neo::runtime",
                local_index,
                network_index,
                "state root index mismatch, cannot compare"
            );
        }
    }
}

fn validate_without_validator(
    state_root: StateRoot,
    local_root: Option<neo_core::UInt256>,
    local_index: u32,
    index: u32,
    network_root: neo_core::UInt256,
    state_store: &Option<Arc<StateStore>>,
) {
    if let Some(local) = local_root {
        if local_index == index {
            if local == network_root {
                info!(
                    target: "neo::runtime",
                    index,
                    root_hash = %local,
                    "STATE ROOT MATCH: local matches network (no signature verification)"
                );
            } else {
                warn!(
                    target: "neo::runtime",
                    index,
                    local_root = %local,
                    network_root = %network_root,
                    "STATE ROOT MISMATCH: local differs from network!"
                );
            }
        }
    }

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

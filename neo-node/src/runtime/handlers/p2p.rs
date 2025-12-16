//! P2P event handler for the runtime.

use crate::executor::BlockExecutorImpl;
use crate::runtime::events::RuntimeEvent;
use crate::state_validator::{StateRootValidator, ValidationResult};
use neo_chain::{BlockIndexEntry, ChainEvent, ChainState};
use neo_core::neo_io::{MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::payloads::Block;
use neo_core::network::p2p::payloads::ExtensiblePayload;
use neo_core::persistence::data_cache::DataCache;
use neo_core::state_service::{StateRoot, StateStore};
use neo_core::IVerifiable;
use neo_p2p::P2PEvent;
use neo_state::{MemoryWorldState, StateTrieManager, WorldState};
use neo_consensus::ConsensusService;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Processes P2P events from the network layer.
#[allow(clippy::too_many_arguments)]
pub async fn process_p2p_events(
    mut rx: mpsc::Receiver<P2PEvent>,
    event_tx: broadcast::Sender<RuntimeEvent>,
    network_magic: u32,
    chain_tx: broadcast::Sender<ChainEvent>,
    chain: Arc<RwLock<ChainState>>,
    state: Arc<RwLock<MemoryWorldState>>,
    state_store: Option<Arc<StateStore>>,
    state_trie: Arc<RwLock<StateTrieManager>>,
    state_validator: Option<Arc<StateRootValidator>>,
    block_executor: Arc<BlockExecutorImpl>,
    consensus: Arc<RwLock<Option<ConsensusService>>>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                handle_p2p_event(
                    event,
                    &event_tx,
                    network_magic,
                    &chain_tx,
                    &chain,
                    &state,
                    &state_store,
                    &state_trie,
                    &state_validator,
                    &block_executor,
                    &consensus,
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
    network_magic: u32,
    chain_tx: &broadcast::Sender<ChainEvent>,
    chain: &Arc<RwLock<ChainState>>,
    state: &Arc<RwLock<MemoryWorldState>>,
    state_store: &Option<Arc<StateStore>>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_validator: &Option<Arc<StateRootValidator>>,
    block_executor: &Arc<BlockExecutorImpl>,
    consensus: &Arc<RwLock<Option<ConsensusService>>>,
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
        P2PEvent::ConsensusReceived { data, from } => {
            handle_consensus_received(network_magic, &data, &from.to_string(), chain, consensus)
                .await;
        }
        P2PEvent::StateRootReceived { data, from } => {
            handle_state_root_received(
                network_magic,
                &data,
                &from.to_string(),
                chain,
                state_trie,
                state_validator,
                state_store,
            )
            .await;
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
                header: block.header.to_array().unwrap_or_default(),
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
    network_magic: u32,
    data: &[u8],
    from: &str,
    chain: &Arc<RwLock<ChainState>>,
    state_trie: &Arc<RwLock<StateTrieManager>>,
    state_validator: &Option<Arc<StateRootValidator>>,
    state_store: &Option<Arc<StateStore>>,
) {
    if data.is_empty() {
        warn!(target: "neo::runtime", from, "received empty StateService message");
        return;
    }

    let mut payload_reader = MemoryReader::new(data);
    let extensible = match ExtensiblePayload::deserialize(&mut payload_reader) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                target: "neo::runtime",
                from,
                error = %e,
                "failed to deserialize StateService extensible payload"
            );
            return;
        }
    };

    if extensible.category != "StateService" {
        debug!(
            target: "neo::runtime",
            from,
            category = %extensible.category,
            "ignoring extensible payload with unexpected category"
        );
        return;
    }

    let height = chain.read().await.height();
    if height < extensible.valid_block_start || height >= extensible.valid_block_end {
        debug!(
            target: "neo::runtime",
            from,
            height,
            start = extensible.valid_block_start,
            end = extensible.valid_block_end,
            "StateService payload outside valid block range"
        );
        return;
    }

    if !verify_extensible_witness(network_magic, &extensible) {
        warn!(
            target: "neo::runtime",
            from,
            "StateService payload witness verification failed"
        );
        return;
    }

    let data = extensible.data;
    if data.is_empty() {
        warn!(target: "neo::runtime", from, "received empty StateService payload data");
        return;
    }

    // Neo.Plugins.StateService prefixes ExtensiblePayload.Data with a message type byte:
    // 0 = Vote, 1 = StateRoot.
    let msg_type = data[0];
    if msg_type != 1 {
        debug!(
            target: "neo::runtime",
            from,
            msg_type,
            "ignoring unsupported StateService message type"
        );
        return;
    }

    let mut reader = MemoryReader::new(&data[1..]);
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
                "failed to deserialize StateRoot from StateService message"
            );
        }
    }
}

async fn handle_consensus_received(
    network_magic: u32,
    data: &[u8],
    from: &str,
    chain: &Arc<RwLock<ChainState>>,
    consensus: &Arc<RwLock<Option<ConsensusService>>>,
) {
    if data.is_empty() {
        warn!(target: "neo::runtime", from, "received empty dBFT message");
        return;
    }

    let mut payload_reader = MemoryReader::new(data);
    let extensible = match ExtensiblePayload::deserialize(&mut payload_reader) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                target: "neo::runtime",
                from,
                error = %e,
                "failed to deserialize dBFT extensible payload"
            );
            return;
        }
    };

    let height = chain.read().await.height();
    if height < extensible.valid_block_start || height >= extensible.valid_block_end {
        debug!(
            target: "neo::runtime",
            from,
            height,
            start = extensible.valid_block_start,
            end = extensible.valid_block_end,
            "dBFT payload outside valid block range"
        );
        return;
    }

    if !verify_extensible_witness(network_magic, &extensible) {
        warn!(
            target: "neo::runtime",
            from,
            "dBFT payload witness verification failed"
        );
        return;
    }

    if extensible.category != "dBFT" {
        debug!(
            target: "neo::runtime",
            from,
            category = %extensible.category,
            "ignoring extensible payload with unexpected category"
        );
        return;
    }

    let signature = match extract_first_push_data(&extensible.witness.invocation_script) {
        Some(sig) => sig,
        None => {
            debug!(target: "neo::runtime", from, "dBFT payload missing signature push");
            return;
        }
    };

    let mut consensus_guard = consensus.write().await;
    let Some(ref mut service) = *consensus_guard else {
        return;
    };

    let network = service.network();
    let payload = match neo_consensus::ConsensusPayload::from_message_bytes(
        network,
        &extensible.data,
        signature,
    ) {
        Ok(p) => p,
        Err(e) => {
            warn!(
                target: "neo::runtime",
                from,
                error = %e,
                "failed to parse dBFT consensus message"
            );
            return;
        }
    };

    let Some(validator) = service
        .context()
        .validators
        .get(payload.validator_index as usize)
    else {
        warn!(
            target: "neo::runtime",
            from,
            validator_index = payload.validator_index,
            "dBFT message with invalid validator index"
        );
        return;
    };

    if extensible.sender != validator.script_hash {
        warn!(
            target: "neo::runtime",
            from,
            validator_index = payload.validator_index,
            sender = %extensible.sender,
            expected = %validator.script_hash,
            "dBFT sender mismatch"
        );
        return;
    }

    // Ensure verification script matches the signature contract for this validator.
    use neo_core::smart_contract::helper::Helper;
    let expected_vs = Helper::signature_redeem_script(&validator.public_key.encoded());
    if extensible.witness.verification_script != expected_vs {
        warn!(
            target: "neo::runtime",
            from,
            validator_index = payload.validator_index,
            "dBFT verification script mismatch"
        );
        return;
    }

    if let Err(e) = service.process_message(payload) {
        debug!(
            target: "neo::runtime",
            from,
            error = %e,
            "failed to process dBFT message"
        );
    }
}

fn extract_first_push_data(script: &[u8]) -> Option<Vec<u8>> {
    if script.is_empty() {
        return None;
    }

    let opcode = script[0];
    let (len, start) = match opcode {
        0x0C => (*script.get(1)? as usize, 2usize), // PUSHDATA1
        0x0D => {
            if script.len() < 3 {
                return None;
            }
            let len = u16::from_le_bytes([script[1], script[2]]) as usize;
            (len, 3usize)
        }
        0x0E => {
            if script.len() < 5 {
                return None;
            }
            let len = u32::from_le_bytes([script[1], script[2], script[3], script[4]]) as usize;
            (len, 5usize)
        }
        0x01..=0x4B => (opcode as usize, 1usize),
        _ => return None,
    };

    let end = start.checked_add(len)?;
    let bytes = script.get(start..end)?;
    Some(bytes.to_vec())
}

fn verify_extensible_witness(network_magic: u32, payload: &ExtensiblePayload) -> bool {
    use neo_core::smart_contract::helper::Helper;
    use neo_crypto::Secp256r1Crypto;

    if payload.witness.invocation_script.is_empty() || payload.witness.verification_script.is_empty()
    {
        return false;
    }

    if neo_core::UInt160::from_script(&payload.witness.verification_script) != payload.sender {
        return false;
    }

    let pushes = extract_all_push_data(&payload.witness.invocation_script);
    if pushes.is_empty() {
        return false;
    }

    let mut tmp = payload.clone();
    let hash = ExtensiblePayload::hash(&mut tmp);
    if hash.is_zero() {
        return false;
    }

    let mut sign_data = Vec::with_capacity(4 + 32);
    sign_data.extend_from_slice(&network_magic.to_le_bytes());
    sign_data.extend_from_slice(&hash.as_bytes());

    let verification_script = &payload.witness.verification_script;
    if Helper::is_signature_contract(verification_script) {
        let sig = &pushes[0];
        if sig.len() != 64 {
            return false;
        }
        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(sig);
        let Some(pubkey) = verification_script.get(2..35) else {
            return false;
        };
        Secp256r1Crypto::verify(&sign_data, &sig_bytes, pubkey).unwrap_or(false)
    } else if Helper::is_multi_sig_contract(verification_script) {
        verify_multisig_witness(&sign_data, &pushes, verification_script)
    } else {
        false
    }
}

fn verify_multisig_witness(sign_data: &[u8], pushes: &[Vec<u8>], script: &[u8]) -> bool {
    use neo_crypto::Secp256r1Crypto;

    let Some((m, pubkeys)) = parse_multisig_contract(script) else {
        return false;
    };

    let signatures: Vec<[u8; 64]> = pushes
        .iter()
        .filter_map(|bytes| {
            if bytes.len() != 64 {
                return None;
            }
            let mut sig = [0u8; 64];
            sig.copy_from_slice(bytes);
            Some(sig)
        })
        .collect();

    if signatures.len() < m {
        return false;
    }

    // NeoVM CheckMultisig: signatures and public keys are matched in order.
    let mut sig_index = 0usize;
    let mut key_index = 0usize;

    while sig_index < m {
        let sig = &signatures[sig_index];
        let mut matched = false;

        while key_index < pubkeys.len() {
            let pubkey = &pubkeys[key_index];
            if Secp256r1Crypto::verify(sign_data, sig, pubkey).unwrap_or(false) {
                matched = true;
                key_index += 1;
                break;
            }
            key_index += 1;
        }

        if !matched {
            return false;
        }

        sig_index += 1;
    }

    true
}

fn parse_multisig_contract(script: &[u8]) -> Option<(usize, Vec<Vec<u8>>)> {
    use neo_core::smart_contract::helper::Helper;

    if !Helper::is_multi_sig_contract(script) || script.len() < 7 {
        return None;
    }

    let m = match script[0] {
        0x51..=0x60 => (script[0] - 0x50) as usize,
        _ => return None,
    };

    // Layout: [PUSHM][pubkeys...][PUSHN][SYSCALL][hash4]
    let n_opcode = *script.get(script.len().saturating_sub(6))?;
    let n = match n_opcode {
        0x51..=0x60 => (n_opcode - 0x50) as usize,
        _ => return None,
    };

    let mut offset = 1usize;
    let end = script.len().saturating_sub(6);
    let mut pubkeys = Vec::with_capacity(n);

    while offset < end {
        let opcode = *script.get(offset)?;
        if opcode != 0x0C {
            return None;
        }
        let len = *script.get(offset + 1)? as usize;
        if len != 33 {
            return None;
        }
        let start = offset + 2;
        let stop = start + 33;
        pubkeys.push(script.get(start..stop)?.to_vec());
        offset = stop;
    }

    if pubkeys.len() != n {
        return None;
    }

    Some((m, pubkeys))
}

fn extract_all_push_data(script: &[u8]) -> Vec<Vec<u8>> {
    let mut offset = 0usize;
    let mut items = Vec::new();

    while offset < script.len() {
        let Some((data, next)) = read_push_data(script, offset) else {
            break;
        };
        items.push(data);
        offset = next;
    }

    items
}

fn read_push_data(script: &[u8], offset: usize) -> Option<(Vec<u8>, usize)> {
    let opcode = *script.get(offset)?;
    let (len, start) = match opcode {
        0x0C => (*script.get(offset + 1)? as usize, offset + 2), // PUSHDATA1
        0x0D => {
            let lo = *script.get(offset + 1)?;
            let hi = *script.get(offset + 2)?;
            (u16::from_le_bytes([lo, hi]) as usize, offset + 3)
        }
        0x0E => {
            let b1 = *script.get(offset + 1)?;
            let b2 = *script.get(offset + 2)?;
            let b3 = *script.get(offset + 3)?;
            let b4 = *script.get(offset + 4)?;
            (u32::from_le_bytes([b1, b2, b3, b4]) as usize, offset + 5)
        }
        0x01..=0x4B => (opcode as usize, offset + 1),
        _ => return None,
    };

    let end = start.checked_add(len)?;
    let bytes = script.get(start..end)?;
    Some((bytes.to_vec(), end))
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

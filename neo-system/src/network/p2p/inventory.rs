//! Inventory handling (inv/getdata/getblocks/mempool/block/tx) via typed ports.
//!
//! Mirrors `neo-core::network::p2p::remote_node::inventory`. The original was a
//! set of `impl RemoteNode` methods reaching through `NeoSystemContext`
//! (`self.system.*`), the actor `ctx.self_ref()`, and an async
//! `self.enqueue_message(..)`. Here every collaborator is a typed port from
//! [`crate::ports`], the peer is an opaque [`PeerHandle`], and handlers are pure
//! synchronous functions: outbound frames are *returned* as a
//! `Vec<NetworkMessage>` for the transport to enqueue, and side-effects go
//! through the ports' fail-closed commands.
//!
//! Fast-sync mode was `self.system.is_fast_sync_mode()`; it is now an explicit
//! `fast_sync: bool` argument so the caller (session actor) owns that policy.

use crate::contains_transaction_type::ContainsTransactionType;
use crate::payloads::inv_payload::MAX_HASHES_COUNT;
use crate::payloads::merkle_block_payload::MerkleBlockPayload;
use crate::payloads::{GetBlockByIndexPayload, GetBlocksPayload, InvPayload};
use crate::peer_messages::normalize_request;
use crate::ports::{PeerHandle, RemoteSessionPorts};
use crate::protocol_message::{NetworkMessage, ProtocolMessage};
use crate::scheduling::BLOCK_INDEX_LOOKAHEAD;
use crate::session_state::SessionState;
use neo_io::Serializable;
use neo_primitives::{InventoryType, UInt160, UInt256};
use neo_protocol::payloads::transaction::MAX_TRANSACTION_SIZE;
use neo_protocol::payloads::{Block, Extensible, Transaction};
use std::time::Instant;

/// Whether a known block should be reprocessed at the persistence frontier.
///
/// A previous attempt at exactly `current_height + 1` may have failed local
/// commit after the payload was accepted; every other known block stays
/// deduplicated.
pub fn should_reprocess_known_block(block_index: u32, current_height: u32) -> bool {
    block_index == current_height.saturating_add(1)
}

/// Handles an `Inv` announcement: filters already-known/persisted hashes and
/// forwards the remainder to the task manager as new tasks.
pub fn on_inv(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    payload: &InvPayload,
    fast_sync: bool,
) {
    if payload.is_empty() {
        return;
    }

    if matches!(
        payload.inventory_type,
        InventoryType::Consensus | InventoryType::Extensible
    ) && fast_sync
    {
        // During fast sync, consensus/extensible inventories can arrive at high
        // frequency and starve block download bandwidth.
        return;
    }

    // Validate inventory count to prevent DoS (matches C# MaxHashesCount).
    if payload.hashes.len() > MAX_HASHES_COUNT {
        return;
    }

    let now = Instant::now();
    let mut hashes = Vec::with_capacity(payload.hashes.len());

    match payload.inventory_type {
        InventoryType::Block => {
            for hash in payload.hashes.iter().copied() {
                if state.should_skip_inventory(&hash) {
                    continue;
                }
                if ports.ledger_query.contains_block(&hash) {
                    continue;
                }
                hashes.push(hash);
            }
        }
        InventoryType::Transaction => {
            for hash in payload.hashes.iter().copied() {
                if state.should_skip_inventory(&hash) {
                    continue;
                }
                if ports.ledger_query.contains_transaction(&hash) {
                    continue;
                }
                hashes.push(hash);
            }
        }
        _ => {
            for hash in payload.hashes.iter().copied() {
                if state.should_skip_inventory(&hash) {
                    continue;
                }
                hashes.push(hash);
            }
        }
    }

    if hashes.is_empty() {
        return;
    }

    for hash in &hashes {
        state.pending_known_hashes.try_add(*hash, now);
    }

    let _ = ports
        .task_manager
        .new_tasks(peer, InvPayload::new(payload.inventory_type, hashes));
}

/// Notifies the task manager that a peer-delivered inventory item completed.
pub fn notify_inventory_completed(
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    hash: UInt256,
    block: Option<Block>,
    block_index: Option<u32>,
) {
    let _ = ports
        .task_manager
        .inventory_completed(peer, hash, block, block_index);
}

/// Handles a broadcast `Transaction`, deduplicating and routing it to
/// pre-verification through the ingress port.
pub fn on_transaction(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    transaction: Transaction,
) {
    // Validate transaction size before processing (matches C# MAX_TRANSACTION_SIZE).
    if transaction.size() > MAX_TRANSACTION_SIZE {
        return;
    }

    let hash = match transaction.try_hash() {
        Ok(hash) => hash,
        Err(_) => return,
    };

    if !state.known_hashes.try_add(hash) {
        return;
    }
    state.pending_known_hashes.remove(&hash);
    notify_inventory_completed(ports, peer, hash, None, None);

    let contains = ports.mempool.contains_transaction(&hash);
    let signer_accounts: Vec<UInt160> = transaction
        .signers()
        .iter()
        .map(|signer| signer.account)
        .collect();
    let has_conflict =
        !signer_accounts.is_empty() && ports.mempool.contains_conflict_hash(&hash, &signer_accounts);

    if contains != ContainsTransactionType::NotExist || has_conflict {
        return;
    }

    if ports
        .ingress
        .enqueue_preverify(transaction, true, peer)
        .is_err()
    {
        // The transaction was not accepted for pre-verification. Drop both dedup
        // records so a later announcement can retry the same transaction.
        state.known_hashes.remove(&hash);
        let _ = ports.task_manager.forget_hash(hash);
    }
}

/// Handles a peer-delivered `Block`, applying the sync window and forwarding it
/// to the chain sink when it is solicited and within range.
pub fn on_block(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    mut block: Block,
    fast_sync: bool,
) {
    let block_idx = block.index();
    let hash = match block.try_hash() {
        Ok(hash) => hash,
        Err(_) => return,
    };
    let current_height = ports.ledger_query.current_index();

    // Already persisted. Release the in-flight index task; do not forward again.
    if block_idx <= current_height {
        state.known_hashes.try_add(hash);
        notify_inventory_completed(ports, peer, hash, None, Some(block_idx));
        return;
    }

    // Limit how far ahead we forward to the chain sink. Must match the task
    // manager's global window so blocks requested by multiple sessions are never
    // silently filtered here. Don't consume known_hashes so the block can be
    // re-processed later when current_height advances into the window.
    let max_ahead = if fast_sync {
        BLOCK_INDEX_LOOKAHEAD
    } else {
        MAX_HASHES_COUNT as u32
    };
    if block_idx > current_height.saturating_add(max_ahead) {
        return;
    }

    if !state.known_hashes.try_add(hash) && !should_reprocess_known_block(block_idx, current_height)
    {
        // Already forwarded. Still release the index task, otherwise a
        // re-request sits in the window until the task timeout. Only the
        // immediate persistence frontier may bypass this dedup.
        notify_inventory_completed(ports, peer, hash, None, Some(block_idx));
        return;
    }

    state.pending_known_hashes.remove(&hash);
    state.last_block_index = block_idx;
    notify_inventory_completed(ports, peer, hash, Some(block.clone()), Some(block_idx));

    let _ = ports
        .chain_sink
        .submit_inventory_block(block, true, false, peer);
}

/// Handles a peer-delivered `Extensible` payload (dBFT / state-service items).
pub fn on_extensible(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    mut payload: Extensible,
) {
    let hash = match payload.try_hash() {
        Ok(hash) => hash,
        Err(_) => return,
    };
    if !state.known_hashes.try_add(hash) {
        return;
    }
    state.pending_known_hashes.remove(&hash);
    notify_inventory_completed(ports, peer, hash, None, None);

    let _ = ports
        .chain_sink
        .submit_inventory_extensible(payload, true, peer);
}

/// Handles a `Mempool` request by announcing the local pool as `Inv` groups.
pub fn on_mempool(ports: &RemoteSessionPorts) -> Vec<NetworkMessage> {
    let hashes = ports.mempool.transaction_hashes();
    if hashes.is_empty() {
        return Vec::new();
    }

    InvPayload::create_group(InventoryType::Transaction, hashes)
        .into_iter()
        .map(|group| NetworkMessage::new(ProtocolMessage::Inv(group)))
        .collect()
}

/// Handles a `GetData` request, replying with the requested payloads (or
/// `MerkleBlock` when a bloom filter is loaded) and a `NotFound` for the rest.
pub fn on_get_data(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    payload: &InvPayload,
) -> Vec<NetworkMessage> {
    if payload.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut not_found = Vec::with_capacity(payload.hashes.len());

    match payload.inventory_type {
        InventoryType::Transaction => {
            for hash in payload.hashes.iter().copied() {
                if !state.sent_hashes.try_add(hash) {
                    continue;
                }
                if let Some(transaction) = ports.mempool.try_get_transaction(&hash) {
                    out.push(NetworkMessage::new(ProtocolMessage::Transaction(
                        transaction,
                    )));
                } else {
                    not_found.push(hash);
                }
            }
        }
        InventoryType::Block => {
            for hash in payload.hashes.iter().copied() {
                if !state.sent_hashes.try_add(hash) {
                    continue;
                }
                if let Some(block) = ports.ledger_query.try_get_block(&hash) {
                    push_block_or_merkle(state, &mut out, block);
                } else {
                    not_found.push(hash);
                }
            }
        }
        InventoryType::Consensus | InventoryType::Extensible => {
            for hash in payload.hashes.iter().copied() {
                if !state.sent_hashes.try_add(hash) {
                    continue;
                }
                if let Some(extensible) = ports
                    .mempool
                    .try_get_relay_extensible(&hash)
                    .or_else(|| ports.mempool.try_get_extensible(&hash))
                {
                    out.push(NetworkMessage::new(ProtocolMessage::Extensible(
                        extensible,
                    )));
                }
            }
        }
    }

    if !not_found.is_empty() {
        for group in InvPayload::create_group(payload.inventory_type, not_found) {
            out.push(NetworkMessage::new(ProtocolMessage::NotFound(group)));
        }
    }

    out
}

/// Pushes either a `MerkleBlock` (when a bloom filter matches) or the full block.
fn push_block_or_merkle(
    state: &SessionState,
    out: &mut Vec<NetworkMessage>,
    block: Block,
) {
    match state.bloom_filter.flags(&block) {
        Some(flags) => match MerkleBlockPayload::try_create(&block, flags) {
            Ok(payload) => out.push(NetworkMessage::new(ProtocolMessage::MerkleBlock(payload))),
            Err(_) => {
                // Unhashable transaction: skip rather than send a bad proof.
            }
        },
        None => out.push(NetworkMessage::new(ProtocolMessage::Block(block))),
    }
}

/// Handles a `GetBlocks` request by announcing the requested hash range as `Inv`.
pub fn on_get_blocks(ports: &RemoteSessionPorts, payload: GetBlocksPayload) -> Vec<NetworkMessage> {
    // Validate that the start hash exists in the ledger (matches C# behavior).
    if ports.ledger_query.try_get_block(&payload.hash_start).is_none() {
        return Vec::new();
    }

    let count = normalize_request(payload.count, MAX_HASHES_COUNT);
    let hashes = ports
        .ledger_query
        .block_hashes_from(&payload.hash_start, count);

    if hashes.is_empty() {
        return Vec::new();
    }

    InvPayload::create_group(InventoryType::Block, hashes)
        .into_iter()
        .map(|group| NetworkMessage::new(ProtocolMessage::Inv(group)))
        .collect()
}

/// Handles a `GetBlockByIndex` request by replying with each block (or its
/// filtered `MerkleBlock`) until the range or the persisted tip is reached.
pub fn on_get_block_by_index(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    payload: GetBlockByIndexPayload,
) -> Vec<NetworkMessage> {
    let count = normalize_request(payload.count, MAX_HASHES_COUNT);
    if count == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    for offset in 0..count {
        let index = payload.index_start.saturating_add(offset as u32);

        let Some(hash) = ports.ledger_query.block_hash_at(index) else {
            break;
        };
        let Some(block) = ports.ledger_query.try_get_block(&hash) else {
            break;
        };

        push_block_or_merkle(state, &mut out, block);
    }

    out
}

/// Handles a `NotFound` reply: clears the pending records and asks the task
/// manager to restart the tasks attributed to this peer.
pub fn on_not_found(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    payload: InvPayload,
) {
    for hash in &payload.hashes {
        state.pending_known_hashes.remove(hash);
    }

    let _ = ports.task_manager.restart_tasks(peer, payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_block_retry_is_limited_to_the_persistence_frontier() {
        assert!(should_reprocess_known_block(8, 7));
        assert!(!should_reprocess_known_block(9, 7));
        assert!(!should_reprocess_known_block(7, 7));
        assert!(!should_reprocess_known_block(0, u32::MAX));
    }
}

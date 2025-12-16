//! Consensus event handler for the runtime.

use crate::p2p_service::BroadcastMessage;
use crate::runtime::events::RuntimeEvent;
use neo_consensus::{ConsensusEvent, ConsensusService};
use neo_mempool::Mempool;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, info, warn};

/// Processes consensus events from the dBFT consensus service.
pub async fn process_consensus_events(
    mut rx: mpsc::Receiver<ConsensusEvent>,
    event_tx: broadcast::Sender<RuntimeEvent>,
    mempool: Arc<RwLock<Mempool>>,
    consensus: Arc<RwLock<Option<ConsensusService>>>,
    p2p_broadcast_tx: Option<broadcast::Sender<BroadcastMessage>>,
    shutdown_rx: &mut broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                handle_consensus_event(
                    event,
                    &event_tx,
                    &mempool,
                    &consensus,
                    &p2p_broadcast_tx,
                ).await;
            }
            _ = shutdown_rx.recv() => {
                info!(target: "neo::runtime", "consensus event processor shutting down");
                break;
            }
        }
    }
}

async fn handle_consensus_event(
    event: ConsensusEvent,
    event_tx: &broadcast::Sender<RuntimeEvent>,
    mempool: &Arc<RwLock<Mempool>>,
    consensus: &Arc<RwLock<Option<ConsensusService>>>,
    p2p_broadcast_tx: &Option<broadcast::Sender<BroadcastMessage>>,
) {
    match event {
        ConsensusEvent::ViewChanged { block_index, old_view, new_view } => {
            handle_view_changed(event_tx, block_index, old_view, new_view);
        }
        ConsensusEvent::BlockCommitted { block_index, block_hash, block_data } => {
            handle_block_committed(block_index, block_hash, &block_data);
        }
        ConsensusEvent::BroadcastMessage(payload) => {
            handle_broadcast_message(payload, p2p_broadcast_tx);
        }
        ConsensusEvent::RequestTransactions { block_index, max_count } => {
            handle_request_transactions(block_index, max_count, mempool, consensus).await;
        }
    }
}

fn handle_view_changed(
    event_tx: &broadcast::Sender<RuntimeEvent>,
    block_index: u32,
    old_view: u8,
    new_view: u8,
) {
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

fn handle_block_committed(
    block_index: u32,
    block_hash: neo_core::UInt256,
    block_data: &neo_consensus::BlockData,
) {
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
    // Block assembly handled by ValidatorService.handle_consensus_event()
}

fn handle_broadcast_message(
    payload: neo_consensus::ConsensusPayload,
    p2p_broadcast_tx: &Option<broadcast::Sender<BroadcastMessage>>,
) {
    info!(
        target: "neo::runtime",
        block_index = payload.block_index,
        validator_index = payload.validator_index,
        view_number = payload.view_number,
        msg_type = ?payload.message_type,
        data_len = payload.data.len(),
        "broadcasting consensus message to peers"
    );

    if let Some(ref tx) = p2p_broadcast_tx {
        let broadcast_msg = BroadcastMessage {
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

async fn handle_request_transactions(
    block_index: u32,
    max_count: usize,
    mempool: &Arc<RwLock<Mempool>>,
    consensus: &Arc<RwLock<Option<ConsensusService>>>,
) {
    info!(
        target: "neo::runtime",
        block_index,
        max_count,
        "consensus requesting transactions"
    );

    let mempool_guard = mempool.read().await;
    let top_txs = mempool_guard.get_top(max_count);
    drop(mempool_guard);

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

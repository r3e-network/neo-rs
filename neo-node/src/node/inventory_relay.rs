//! Inbound peer-inventory relay adapter.
//!
//! The network crate decodes peer inventory but does not own the ledger,
//! mempool, consensus, or state-root services. This module is the node-layer
//! adapter that batches blocks into the blockchain service and forwards
//! transaction/extensible payload side effects to their owning services.

use std::sync::Arc;

pub(super) const FAST_SYNC_BURST_CAPACITY: usize = 4096;
pub(super) const FAST_SYNC_BLOCK_BATCH_SIZE: usize = 500;
pub(super) const FAST_SYNC_BLOCK_BATCH_FLUSH_MS: u64 = 5;

pub(super) async fn flush_inventory_block_batch(
    blockchain: &neo_blockchain::BlockchainHandle,
    pending_blocks: &mut Vec<Arc<neo_payloads::Block>>,
) {
    if pending_blocks.is_empty() {
        return;
    }
    let blocks = std::mem::take(pending_blocks);
    let _ = blockchain
        .submit_inventory_blocks(blocks, true, false)
        .await;
}

pub(super) async fn handle_inbound_inventory_item(
    item: neo_network::InboundInventory,
    blockchain: &neo_blockchain::BlockchainHandle,
    relay: &neo_network::NetworkHandle,
    consensus_decode: &Option<(
        Arc<parking_lot::RwLock<Vec<neo_consensus::ValidatorInfo>>>,
        u32,
    )>,
    consensus_inbound_tx: &Option<
        tokio::sync::mpsc::Sender<neo_consensus::messages::ConsensusPayload>,
    >,
    consensus_tx_feed_tx: &Option<tokio::sync::mpsc::Sender<neo_primitives::UInt256>>,
    state_root_inbound_tx: &Option<tokio::sync::mpsc::Sender<neo_payloads::ExtensiblePayload>>,
    pending_blocks: &mut Vec<Arc<neo_payloads::Block>>,
) {
    use neo_network::InboundInventory;

    match item {
        InboundInventory::Block(block) => {
            pending_blocks.push(block);
            if pending_blocks.len() >= FAST_SYNC_BLOCK_BATCH_SIZE {
                flush_inventory_block_batch(blockchain, pending_blocks).await;
            }
        }
        InboundInventory::Transaction(tx) => {
            flush_inventory_block_batch(blockchain, pending_blocks).await;
            // Admit the peer's transaction to the mempool; the C# `Transaction.Verify`
            // pipeline runs inside the blockchain service. On a fresh accept (Succeed),
            // re-announce it to peers via `Inv` so it propagates.
            if let Ok(reply) = blockchain.add_transaction((*tx).clone()).await {
                if reply.result.is_success() {
                    let _ = relay
                        .broadcast_inv(neo_network::InventoryType::Transaction, vec![reply.hash])
                        .await;
                    // C# `ConsensusService.OnTransaction`: a freshly-accepted
                    // transaction is fed to the consensus state machine so a
                    // backup missing a proposal transaction can resume the round
                    // when it lands rather than degrading to a view change.
                    if let Some(feed) = consensus_tx_feed_tx {
                        let _ = feed.send(reply.hash).await;
                    }
                }
            }
        }
        InboundInventory::Extensible(payload) => {
            flush_inventory_block_batch(blockchain, pending_blocks).await;
            // dBFT consensus messages: when this node is a validator, decode +
            // authenticate the payload and feed it to the consensus driver.
            // (`extensible_to_consensus` returns `None` for non-dBFT or spoofed payloads.)
            if let (Some((validators, network_magic)), Some(tx)) =
                (consensus_decode, consensus_inbound_tx)
            {
                let cp = {
                    let validators = validators.read();
                    crate::consensus::extensible_to_consensus(&payload, *network_magic, &validators)
                };
                if let Some(cp) = cp {
                    let _ = tx.send(cp).await;
                }
            }
            // StateService votes/roots: feed to the state-root driver, which
            // decodes + authenticates them (it verifies signed roots against the
            // designated StateValidators before persisting).
            if let Some(tx) = state_root_inbound_tx {
                if payload.category == neo_state_service::STATE_SERVICE_CATEGORY {
                    let _ = tx.send((*payload).clone()).await;
                }
            }
            // Cache + relay through the blockchain service regardless
            // (peers that are validators consume it; we relay it on).
            let _ = blockchain
                .submit_inventory_extensible((*payload).clone(), true)
                .await;
        }
    }
}

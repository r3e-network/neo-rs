use super::helpers::{
    compute_header_hash, compute_merkle_root, compute_next_consensus_address, current_timestamp,
    generate_nonce,
};
use super::{ConsensusEvent, ConsensusService};
use crate::messages::PrepareRequestMessage;
use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use tracing::info;

impl ConsensusService {
    /// Initiates a block proposal (called when we're the primary)
    pub(super) fn initiate_proposal(&mut self, _timestamp: u64) -> ConsensusResult<()> {
        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            "Initiating block proposal as primary"
        );

        // Request transactions from mempool
        self.send_event(ConsensusEvent::RequestTransactions {
            block_index: self.context.block_index,
            max_count: 500, // Max transactions per block
        })?;

        Ok(())
    }

    /// Called when transactions are received from mempool
    pub fn on_transactions_received(&mut self, tx_hashes: Vec<UInt256>) -> ConsensusResult<()> {
        if !self.running || !self.context.is_primary() {
            return Ok(());
        }

        let timestamp = current_timestamp();
        let nonce = generate_nonce();

        // Store proposal data
        self.context.proposed_timestamp = timestamp;
        self.context.proposed_tx_hashes = tx_hashes.clone();
        self.context.nonce = nonce;

        // Create and broadcast PrepareRequest
        let msg = PrepareRequestMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
            self.context.version,
            self.context.prev_hash,
            timestamp,
            nonce,
            tx_hashes,
        );

        let payload = self.create_payload(ConsensusMessageType::PrepareRequest, msg.serialize())?;

        // Cache the primary PrepareRequest payload hash (ExtensiblePayload.Hash).
        if let Ok(hash) = self.dbft_payload_hash(&payload) {
            self.context.preparation_hash = Some(hash);
        }

        // Compute block header hash for commit signatures.
        let merkle_root = compute_merkle_root(&self.context.proposed_tx_hashes);
        let next_consensus = compute_next_consensus_address(&self.context.validators);
        self.context.proposed_block_hash = Some(compute_header_hash(
            self.context.version,
            self.context.prev_hash,
            merkle_root,
            timestamp,
            nonce,
            self.context.block_index,
            self.context.primary_index(),
            next_consensus,
        ));

        self.broadcast(payload)?;

        // Mark that we've sent the prepare request
        self.context.prepare_request_received = true;

        Ok(())
    }
}

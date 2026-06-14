use super::helpers::{
    ConsensusBlockFields, InvocationScript, current_timestamp, generate_nonce,
    prepare_request_timestamp,
};
use super::{ConsensusEvent, ConsensusService};
use crate::messages::PrepareRequestMessage;
use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use tracing::info;

impl ConsensusService {
    /// Asks the node/mempool for the transactions to include in the primary's
    /// delayed `PrepareRequest`.
    pub(super) fn initiate_proposal(&mut self, timestamp: u64) -> ConsensusResult<()> {
        if self.context.transaction_request_sent {
            return Ok(());
        }
        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            "Requesting transactions for primary proposal"
        );

        // Request transactions from mempool
        self.context.transaction_request_sent = true;
        self.context.transaction_request_sent_at = Some(timestamp);
        self.send_event(ConsensusEvent::RequestTransactions {
            block_index: self.context.block_index,
            max_count: 500, // Max transactions per block
        })?;

        Ok(())
    }

    /// Called when transactions are received from mempool
    pub fn on_transactions_received(&mut self, tx_hashes: Vec<UInt256>) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }
        if self.context.not_accepting_payloads_due_to_view_changing() {
            return Ok(());
        }

        if self.context.is_primary() {
            let now = current_timestamp();
            self.context.transaction_request_sent = true;
            if self.context.transaction_request_sent_at.is_none() {
                self.context.transaction_request_sent_at = Some(now);
            }

            let timestamp = prepare_request_timestamp(now, self.context.previous_block_timestamp);
            let nonce = generate_nonce();

            // Store proposal data
            self.context.proposed_timestamp = timestamp;
            self.context.proposed_tx_hashes = tx_hashes.clone();
            self.context.mark_available_transactions(tx_hashes.clone());
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

            let payload =
                self.create_payload(ConsensusMessageType::PrepareRequest, msg.serialize())?;

            // Cache the primary PrepareRequest payload hash (ExtensiblePayload.Hash).
            if let Ok(hash) = self.dbft_payload_hash(&payload) {
                self.context.preparation_hash = Some(hash);
            }
            self.context.prepare_request_invocation = if payload.witness.is_empty() {
                None
            } else {
                Some(InvocationScript::invocation_script_from_signature(
                    &payload.witness,
                ))
            };

            // Compute block header hash for commit signatures.
            let merkle_root =
                ConsensusBlockFields::compute_merkle_root(&self.context.proposed_tx_hashes);
            self.context.proposed_block_hash = Some(ConsensusBlockFields::compute_header_hash(
                self.context.version,
                self.context.prev_hash,
                merkle_root,
                timestamp,
                nonce,
                self.context.block_index,
                self.context.primary_index(),
                self.context.next_consensus,
            ));

            self.broadcast(payload)?;

            // Mark that we've sent the prepare request
            self.context.prepare_request_received = true;

            return Ok(());
        }

        if !self.context.prepare_request_received {
            return Ok(());
        }

        if self.context.proposed_tx_hashes.is_empty() {
            self.send_prepare_response()?;
            return Ok(());
        }

        self.context
            .mark_available_transactions(tx_hashes.iter().copied());
        let available: std::collections::HashSet<UInt256> = tx_hashes.into_iter().collect();
        let all_present = self
            .context
            .proposed_tx_hashes
            .iter()
            .all(|hash| available.contains(hash));
        if all_present {
            self.send_prepare_response()?;
        }

        Ok(())
    }
}

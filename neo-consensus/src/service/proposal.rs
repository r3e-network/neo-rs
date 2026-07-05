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
            max_count: self.max_transactions_per_block as usize,
            invalid_tx_hashes: self.context.invalid_tx_hashes_over_f(),
        })?;

        Ok(())
    }

    /// Called when transactions are received from mempool
    pub async fn on_transactions_received(
        &mut self,
        tx_hashes: Vec<UInt256>,
    ) -> ConsensusResult<()> {
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

            let tx_hashes: Vec<UInt256> = tx_hashes
                .into_iter()
                .take(self.max_transactions_per_block as usize)
                .collect();

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

            let payload = self
                .create_payload(ConsensusMessageType::PrepareRequest, msg.serialize())
                .await?;

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
            self.revalidate_current_view_commits();

            self.broadcast(payload)?;

            // Mark that we've sent the prepare request
            self.context.prepare_request_received = true;

            return Ok(());
        }

        if !self.context.prepare_request_received {
            return Ok(());
        }

        if self.context.proposed_tx_hashes.is_empty() {
            self.send_prepare_response().await?;
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
            self.send_prepare_response().await?;
        }

        Ok(())
    }

    /// Late-transaction feed: a single transaction arrived (from the mempool /
    /// peer relay) AFTER this backup received the primary's `PrepareRequest`.
    ///
    /// This is the port of C# `ConsensusService.OnTransaction(Transaction)`
    /// (ConsensusService.cs). C# subscribes the consensus service to the
    /// mempool relay and, for each arriving transaction the current proposal is
    /// still waiting for, records it in `context.Transactions` and re-runs the
    /// gated preparation check — so when the LAST missing transaction arrives
    /// the round proceeds exactly as if all transactions had been present at
    /// `PrepareRequest` time (sends the `PrepareResponse`, then re-checks the
    /// commit threshold). Without this a backup that lacked a proposal
    /// transaction could only wait out the view timer and view-change, losing
    /// liveness on every incompletely-propagated round.
    ///
    /// Guards mirror C# `OnTransaction` exactly:
    /// - only a backup that has received a `PrepareRequest` acts
    ///   (`IsBackup && RequestSentOrReceived`);
    /// - skip while a view change is in progress
    ///   (`NotAcceptingPayloadsDueToViewChanging`);
    /// - skip if the block was already produced (`BlockSent`, modeled as
    ///   `state == Committed`);
    /// - skip a transaction already recorded (`context.Transactions.ContainsKey`)
    ///   or one not referenced by the proposal (`!TransactionHashes.Contains`).
    ///
    /// Note: C# additionally short-circuits on `ResponseSent`. We deliberately
    /// do NOT, because a backup may have sent its `PrepareResponse` yet stalled
    /// at the commit gate (`check_prepare_responses` returns early while
    /// `has_missing_proposed_transactions()`); feeding the final missing
    /// transaction must still re-drive the commit check. `send_prepare_response`
    /// is idempotent (it early-returns once our response is recorded), so
    /// re-entry never double-sends.
    pub async fn on_transaction(&mut self, tx_hash: UInt256) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }
        // IsBackup && RequestSentOrReceived (C# `OnTransaction` guard).
        if !self.context.is_backup() || !self.context.prepare_request_received {
            return Ok(());
        }
        // NotAcceptingPayloadsDueToViewChanging (C# `OnTransaction` guard).
        if self.context.not_accepting_payloads_due_to_view_changing() {
            return Ok(());
        }
        // BlockSent (C# `OnTransaction` guard) — modeled as Committed state.
        if self.context.state == crate::context::ConsensusState::Committed {
            return Ok(());
        }
        // context.Transactions.ContainsKey(hash) — already have it.
        if self.context.has_available_transaction(&tx_hash) {
            return Ok(());
        }
        // !context.TransactionHashes.Contains(hash) — not part of this proposal.
        if !self.context.is_proposed_transaction(&tx_hash) {
            return Ok(());
        }

        // C# `AddTransaction`: record it, then `CheckPrepareResponse`.
        if !self.context.mark_transaction_available(tx_hash) {
            return Ok(());
        }

        // C# `CheckPrepareResponse`: once every proposed transaction is present,
        // send our `PrepareResponse` (idempotent) and re-check the commit
        // threshold. If transactions are still missing, both calls no-op via
        // their internal gates and we simply wait for the next arrival.
        if !self.context.has_missing_proposed_transactions() {
            self.send_prepare_response().await?;
            // `send_prepare_response` already calls `check_prepare_responses`
            // when it sends, but not when our response was previously recorded.
            // Re-drive it explicitly so a backup that already responded but
            // stalled on missing transactions can now sign its Commit.
            self.check_prepare_responses().await?;
        }

        Ok(())
    }
}

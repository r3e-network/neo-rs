use super::super::helpers::{ConsensusBlockFields, InvocationScript, current_timestamp};
use super::super::{ConsensusEvent, ConsensusService};
use crate::context::ConsensusState;
use crate::messages::{
    CommitMessage, ConsensusPayload, PrepareRequestMessage, PrepareResponseMessage,
};
use crate::{ChangeViewReason, ConsensusError, ConsensusMessageType, ConsensusResult};
use tracing::{debug, info, warn};

impl<S> ConsensusService<S>
where
    S: crate::ConsensusSigner,
{
    /// Handles `PrepareRequest` message
    pub(in crate::service) async fn on_prepare_request(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        if self.context.not_accepting_payloads_due_to_view_changing() {
            return Ok(());
        }

        // Verify sender is the primary
        let expected_primary = self.context.primary_index();
        if payload.validator_index != expected_primary {
            return Err(ConsensusError::InvalidPrimary {
                expected: expected_primary,
                got: payload.validator_index,
            });
        }

        // Verify the primary's signature
        // SECURITY: Require non-empty witness and valid signature
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "PrepareRequest missing witness"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareRequest missing witness",
            ));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "PrepareRequest signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareRequest signature invalid",
            ));
        }

        // Check if we already received a prepare request
        if self.context.prepare_request_received {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        info!(
            block_index = self.context.block_index,
            view = self.context.view_number,
            primary = payload.validator_index,
            "Received PrepareRequest"
        );

        let expected_primary = self.context.primary_index();
        let prepare_request = PrepareRequestMessage::deserialize_body(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;
        prepare_request.validate(expected_primary, self.max_transactions_per_block)?;
        if prepare_request.version != self.context.version {
            return Err(ConsensusError::invalid_proposal(
                "PrepareRequest version does not match block context",
            ));
        }
        if prepare_request.prev_hash != self.context.prev_hash {
            return Err(ConsensusError::invalid_proposal(
                "PrepareRequest prev_hash does not match block context",
            ));
        }
        if prepare_request.timestamp <= self.context.previous_block_timestamp {
            return Err(ConsensusError::invalid_proposal(
                "PrepareRequest timestamp must be greater than previous block timestamp",
            ));
        }
        let max_timestamp = current_timestamp()
            .saturating_add(self.context.prepare_request_delay().saturating_mul(8));
        if prepare_request.timestamp > max_timestamp {
            return Err(ConsensusError::invalid_proposal(
                "PrepareRequest timestamp too far in the future",
            ));
        }

        // Mark prepare request as received and store proposal fields.
        self.context.prepare_request_received = true;
        self.context.prepare_request_invocation = if payload.witness.is_empty() {
            None
        } else {
            Some(InvocationScript::invocation_script_from_signature(
                &payload.witness,
            ))
        };
        self.context.version = prepare_request.version;
        self.context.prev_hash = prepare_request.prev_hash;
        self.context.proposed_timestamp = prepare_request.timestamp;
        self.context.nonce = prepare_request.nonce;
        self.context.proposed_tx_hashes = prepare_request.transaction_hashes;
        self.context.available_tx_hashes.clear();

        // Cache PrepareRequest payload hash (ExtensiblePayload.Hash) for PrepareResponse.
        self.context.preparation_hash = Some(self.dbft_payload_hash(payload)?);
        if let Some(expected) = self.context.preparation_hash {
            self.context
                .prepare_responses
                .retain(|idx, _| self.context.prepare_response_hashes.get(idx) == Some(&expected));
            self.context
                .prepare_response_hashes
                .retain(|_, hash| *hash == expected);
        }

        // Calculate block header hash from proposal data (for commit signatures).
        let merkle_root =
            ConsensusBlockFields::compute_merkle_root(&self.context.proposed_tx_hashes);
        let block_hash = ConsensusBlockFields::compute_header_hash(
            self.context.version,
            self.context.prev_hash,
            merkle_root,
            self.context.proposed_timestamp,
            self.context.nonce,
            self.context.block_index,
            self.context.primary_index(),
            self.context.next_consensus,
        );
        self.context.proposed_block_hash = Some(block_hash);
        self.revalidate_current_view_commits();

        // C# OnPrepareRequestReceived / CheckPrepareRequest: a prepare request
        // received with success extends the change-view timer (factor 2).
        self.context.extend_timer_by_factor(2);

        // If there are no transactions, respond immediately.
        if self.context.proposed_tx_hashes.is_empty() {
            self.send_prepare_response().await?;
        } else if self.context.is_backup() {
            self.send_event(ConsensusEvent::RequestProposalTransactions {
                block_index: self.context.block_index,
                transaction_hashes: self.context.proposed_tx_hashes.clone(),
            })?;
        }

        Ok(())
    }

    /// Handles `PrepareResponse` message
    pub(in crate::service) async fn on_prepare_response(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        if self.context.not_accepting_payloads_due_to_view_changing() {
            return Ok(());
        }

        // The primary's preparation IS its PrepareRequest, not a PrepareResponse.
        // C# keys PreparationPayloads by validator index and sets the primary's
        // slot from the PrepareRequest, so a PrepareResponse from the primary is
        // ignored. Drop it here so the primary is not double-counted toward M.
        if payload.validator_index == self.context.primary_index() {
            return Ok(());
        }

        // Check if we already have this response
        if self
            .context
            .prepare_responses
            .contains_key(&payload.validator_index)
        {
            return Err(ConsensusError::AlreadyReceived(payload.validator_index));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received PrepareResponse"
        );

        // Verify the payload signature
        // SECURITY: Require non-empty witness and valid signature
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "PrepareResponse missing witness"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareResponse missing witness",
            ));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "PrepareResponse signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "PrepareResponse signature invalid",
            ));
        }

        let msg = PrepareResponseMessage::deserialize_body(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;

        // Verify PreparationHash matches the primary PrepareRequest payload hash (C# behavior).
        if let Some(expected) = self.context.preparation_hash {
            msg.validate(&expected)?;
        }

        // Add the response
        let invocation_script =
            InvocationScript::invocation_script_from_signature(&payload.witness);
        self.context.add_prepare_response(
            payload.validator_index,
            invocation_script,
            Some(msg.preparation_hash),
        )?;

        // C# OnPrepareResponseReceived: a prepare response received with success
        // extends the change-view timer (factor 2).
        self.context.extend_timer_by_factor(2);

        self.check_prepare_responses().await?;

        Ok(())
    }

    /// Sends our `PrepareResponse` if needed and not already sent.
    ///
    /// This is the choke point for C# `ConsensusService.CheckPrepareResponse`
    /// (called once every proposed transaction is present). Before a backup
    /// signs and broadcasts its `PrepareResponse` it re-checks the proposed
    /// block against the dBFT block-size and block-system-fee policy limits, in
    /// that order, exactly as C# does — a backup must NOT endorse a block a
    /// (malicious or buggy) primary packed beyond `MaxBlockSize` /
    /// `MaxBlockSystemFee`, and instead requests a view change with
    /// `BlockRejectedByPolicy`.
    pub(in crate::service) async fn send_prepare_response(&mut self) -> ConsensusResult<()> {
        if self.context.is_primary() {
            return Ok(());
        }

        let my_index = match self.context.my_index {
            Some(index) => index,
            None => return Ok(()),
        };

        if self.context.prepare_responses.contains_key(&my_index) {
            return Ok(());
        }

        // C# `CheckPrepareResponse`: policy checks run only once the whole
        // proposal is present (`TransactionHashes.Length == Transactions.Count`).
        // Every caller already gates on this, but guard here too so a partial
        // proposal never trips a spurious rejection on incomplete metrics.
        if !self.context.has_missing_proposed_transactions() {
            // Check maximum block size via block policy (C# check #1).
            let expected_block_size = self.context.expected_block_size();
            if expected_block_size > self.context.max_block_size as usize {
                warn!(
                    block_index = self.context.block_index,
                    expected_block_size,
                    max_block_size = self.context.max_block_size,
                    "Rejected block: the size exceeds the policy"
                );
                self.request_change_view(
                    ChangeViewReason::BlockRejectedByPolicy,
                    current_timestamp(),
                )
                .await?;
                return Ok(());
            }
            // Check maximum block system fee via block policy (C# check #2).
            let expected_block_system_fee = self.context.expected_block_system_fee();
            if expected_block_system_fee > self.context.max_block_system_fee {
                warn!(
                    block_index = self.context.block_index,
                    expected_block_system_fee,
                    max_block_system_fee = self.context.max_block_system_fee,
                    "Rejected block: the system fee exceeds the policy"
                );
                self.request_change_view(
                    ChangeViewReason::BlockRejectedByPolicy,
                    current_timestamp(),
                )
                .await?;
                return Ok(());
            }
        }

        // C# `CheckPrepareResponse`: timeout extension due to the prepare
        // response we are about to send (`ExtendTimerByFactor(2)`), applied
        // immediately before broadcasting the response.
        self.context.extend_timer_by_factor(2);

        let preparation_hash = self.context.preparation_hash.unwrap_or_default();
        let response = PrepareResponseMessage::new(
            self.context.block_index,
            self.context.view_number,
            my_index,
            preparation_hash,
        );

        let response_payload = self
            .create_payload(ConsensusMessageType::PrepareResponse, response.serialize())
            .await?;
        let my_witness = response_payload.witness.clone();
        let invocation_script = InvocationScript::invocation_script_from_signature(&my_witness);
        self.broadcast(response_payload)?;

        // Add our own response
        self.context
            .add_prepare_response(my_index, invocation_script, Some(preparation_hash))?;

        self.check_prepare_responses().await?;

        Ok(())
    }

    /// Checks if we have enough prepare responses to send commit
    pub(in crate::service) async fn check_prepare_responses(&mut self) -> ConsensusResult<()> {
        // C# calls `CheckPreparations` (→ send Commit) only when
        // `context.RequestSentOrReceived` — i.e. the PrepareRequest has been sent
        // (primary) or received (backup). Without this gate, M PrepareResponses
        // arriving BEFORE the PrepareRequest (network reordering, or a colluding
        // majority) would pass — `proposed_tx_hashes` is still empty so
        // `has_missing_proposed_transactions()` is vacuously false — and the node
        // would commit-sign the DEFAULT (zero) block hash, burning its commit for
        // the view. `prepare_request_received` is set for both the sending primary
        // and a receiving backup, matching C# `RequestSentOrReceived`.
        if !self.context.prepare_request_received {
            return Ok(());
        }

        if !self.context.has_enough_prepare_responses() {
            return Ok(());
        }

        // C# ConsensusService.CheckPreparations requires BOTH M preparations AND
        // that every proposed transaction is available before signing a Commit.
        // Without this gate a backup could commit-sign a block whose transactions
        // it never received/validated (mirrors check_commits in commit.rs).
        if self.context.has_missing_proposed_transactions() {
            debug!(
                block_index = self.context.block_index,
                missing = self
                    .context
                    .proposed_tx_hashes
                    .len()
                    .saturating_sub(self.context.available_tx_hashes.len()),
                "PrepareResponse threshold reached before all proposal transactions are available"
            );
            return Ok(());
        }

        if self.context.state == ConsensusState::Committed {
            return Ok(());
        }

        // Idempotency / crash-safety: if we have already signed our own Commit
        // for this view (e.g. resumed from the recovery log after a restart, or
        // re-entered via `resume`), we must NOT sign again. Re-signing would hit
        // `add_commit` -> `AlreadyReceived` and, more importantly, must never
        // produce a *second* commit at the same (height, view). C# `MakeCommit`
        // returns the cached commit payload when `CommitSent`; we simply stop.
        if self
            .context
            .my_index
            .is_some_and(|idx| self.context.commits.contains_key(&idx))
        {
            return Ok(());
        }

        // We have enough responses - send Commit
        info!(
            block_index = self.context.block_index,
            responses = self.context.prepare_responses.len(),
            "Enough PrepareResponses received, sending Commit"
        );

        let block_hash = self.context.proposed_block_hash.unwrap_or_default();
        let signature = self.sign_block_hash(&block_hash).await?;

        let my_index = self.my_index()?;
        let commit = CommitMessage::new(
            self.context.block_index,
            self.context.view_number,
            my_index,
            signature.clone(),
        );

        let payload = self
            .create_payload(ConsensusMessageType::Commit, commit.serialize())
            .await?;
        let commit_witness = payload.witness.clone();
        let commit_invocation = InvocationScript::invocation_script_from_signature(&commit_witness);

        // Record our own commit in the context BEFORE persisting so the saved
        // recovery log already reflects the commit we are about to broadcast.
        if !commit_witness.is_empty() {
            self.context
                .commit_invocations
                .insert(my_index, commit_invocation);
        }
        self.context
            .add_commit(my_index, self.context.view_number, signature)?;

        // C# `ConsensusService.CheckPreparations` calls `context.Save()` here —
        // immediately before `localNode.Tell(payload)` — so the recovery log is
        // durable BEFORE the Commit leaves this node. A crash after broadcast
        // then resumes from a state that already records this commit and cannot
        // sign a different block at the same (height, view). If persistence
        // fails, the error propagates and the Commit is NOT broadcast, matching
        // C# where a throwing `store.PutSync` aborts before the `Tell`.
        self.save_context_if_configured()?;

        self.broadcast(payload)?;

        self.check_commits()?;

        Ok(())
    }
}

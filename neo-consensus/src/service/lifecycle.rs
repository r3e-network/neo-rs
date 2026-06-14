use super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::service::helpers::ConsensusBlockFields;
use crate::{ChangeViewReason, ConsensusError, ConsensusMessageType, ConsensusResult};
use neo_primitives::{UInt160, UInt256};
use tracing::{debug, info};

impl ConsensusService {
    /// Starts consensus for a new block
    pub fn start(
        &mut self,
        block_index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        version: u32,
    ) -> ConsensusResult<()> {
        self.start_with_previous_timestamp(block_index, timestamp, prev_hash, 0, version)
    }

    /// Starts consensus for a new block with the previous header timestamp.
    ///
    /// The previous timestamp is required for C# Neo v3.10.0 parity:
    /// `MakePrepareRequest` clamps the proposal timestamp to at least
    /// `PrevHeader.Timestamp + 1`.
    pub fn start_with_previous_timestamp(
        &mut self,
        block_index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        previous_block_timestamp: u64,
        version: u32,
    ) -> ConsensusResult<()> {
        let next_consensus =
            ConsensusBlockFields::compute_next_consensus_address(&self.context.validators);
        self.start_with_block_context(
            block_index,
            timestamp,
            prev_hash,
            previous_block_timestamp,
            next_consensus,
            version,
        )
    }

    /// Starts consensus for a new block with the full header context.
    ///
    /// `next_consensus` is the C# `ConsensusContext.Block.Header.NextConsensus`
    /// value computed during `Reset(0)`.
    pub fn start_with_block_context(
        &mut self,
        block_index: u32,
        timestamp: u64,
        prev_hash: UInt256,
        previous_block_timestamp: u64,
        next_consensus: UInt160,
        version: u32,
    ) -> ConsensusResult<()> {
        if self.context.my_index.is_none() {
            return Err(ConsensusError::NotValidator);
        }

        info!(block_index, "Starting consensus");
        self.context.reset_for_new_block(block_index, timestamp);
        self.context.prev_hash = prev_hash;
        self.context.previous_block_timestamp = previous_block_timestamp;
        self.context.next_consensus = next_consensus;
        self.context.version = version;
        self.running = true;

        Ok(())
    }

    /// Resumes consensus from a recovered context.
    ///
    /// This restores transient fields that are not persisted and continues the round.
    pub fn resume(
        &mut self,
        timestamp: u64,
        prev_hash: UInt256,
        version: u32,
    ) -> ConsensusResult<()> {
        let next_consensus =
            ConsensusBlockFields::compute_next_consensus_address(&self.context.validators);
        self.resume_with_next_consensus(timestamp, prev_hash, next_consensus, version)
    }

    /// Resumes consensus from a recovered context with the header `NextConsensus`
    /// supplied by the caller.
    pub fn resume_with_next_consensus(
        &mut self,
        timestamp: u64,
        prev_hash: UInt256,
        next_consensus: UInt160,
        version: u32,
    ) -> ConsensusResult<()> {
        if self.context.my_index.is_none() {
            return Err(ConsensusError::NotValidator);
        }

        self.context.prev_hash = prev_hash;
        self.context.next_consensus = next_consensus;
        self.context.version = version;
        self.context.view_start_time = timestamp;
        self.context.state = if self.context.is_primary() {
            crate::context::ConsensusState::Primary
        } else {
            crate::context::ConsensusState::Backup
        };
        self.running = true;

        self.check_prepare_responses()?;
        self.check_commits()?;

        Ok(())
    }

    /// Processes a consensus message
    pub fn process_message(&mut self, payload: ConsensusPayload) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }

        // Compute ExtensiblePayload.Hash for deduplication (replay attack prevention).
        // C# DBFTPlugin caches messages by ExtensiblePayload.Hash (SHA256 of unsigned payload).
        let msg_hash = self.dbft_payload_hash(&payload)?;

        // Check if we've already seen this message (replay attack prevention)
        if self.context.has_seen_message(&msg_hash) {
            debug!(
                block_index = payload.block_index,
                validator = payload.validator_index,
                msg_type = ?payload.message_type,
                "Ignoring duplicate message (already processed)"
            );
            return Ok(());
        }

        // Validate block index
        if payload.block_index != self.context.block_index {
            // Message for a future block - queue or ignore per dBFT spec
            if payload.block_index > self.context.block_index {
                debug!(
                    expected = self.context.block_index,
                    got = payload.block_index,
                    "Received message for future block"
                );
                return Ok(());
            }
            return Err(ConsensusError::WrongBlock {
                expected: self.context.block_index,
                got: payload.block_index,
            });
        }

        // Update last seen message for this validator
        // This is used to track failed/lost nodes for recovery logic
        self.context
            .update_last_seen_message(payload.validator_index, payload.block_index);

        // Validate view number (ChangeView and Recovery messages can be for other views).
        #[allow(clippy::collapsible_if)]
        if !matches!(
            payload.message_type,
            ConsensusMessageType::ChangeView
                | ConsensusMessageType::RecoveryRequest
                | ConsensusMessageType::RecoveryMessage
        ) && payload.view_number != self.context.view_number
        {
            if payload.message_type != ConsensusMessageType::Commit {
                return Ok(());
            }
        }

        match payload.message_type {
            ConsensusMessageType::PrepareRequest => {
                self.on_prepare_request(&payload)?;
            }
            ConsensusMessageType::PrepareResponse => {
                self.on_prepare_response(&payload)?;
            }
            ConsensusMessageType::Commit => {
                self.on_commit(&payload)?;
            }
            ConsensusMessageType::ChangeView => {
                self.on_change_view(&payload)?;
            }
            ConsensusMessageType::RecoveryRequest => {
                self.on_recovery_request(&payload)?;
            }
            ConsensusMessageType::RecoveryMessage => {
                self.on_recovery_message(&payload)?;
            }
        }

        // Record the payload as seen ONLY after its witness/signature has been
        // verified by the per-type handler above. C# reaches OnConsensusPayload
        // only for relay-verified payloads (ExtensiblePayload.VerifyWitnesses);
        // a forged-witness payload must not poison the dedup cache and silence
        // the genuine signed message (same unsigned bytes -> same hash).
        self.context.mark_message_seen(&msg_hash);

        Ok(())
    }

    /// Handles timer tick for timeout detection
    pub fn on_timer_tick(&mut self, timestamp: u64) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }

        if self
            .context
            .my_index
            .is_some_and(|idx| self.context.commits.contains_key(&idx))
        {
            let should_resend = self.context.commit_recovery_sent_at.map_or(
                self.context.is_timed_out(timestamp),
                |sent_at| {
                    timestamp >= sent_at.saturating_add(self.context.commit_recovery_resend_delay())
                },
            );

            if should_resend {
                self.resend_recovery_message()?;
                self.context.commit_recovery_sent_at = Some(timestamp);
            }
            return Ok(());
        }

        if self.context.is_primary() {
            let prepare_deadline = self
                .context
                .view_start_time
                .saturating_add(self.context.prepare_request_delay());
            let primary_timeout = self
                .context
                .transaction_request_sent_at
                .unwrap_or(prepare_deadline)
                .saturating_add(self.context.prepare_request_follow_up_delay());

            if !self.context.prepare_request_received {
                if timestamp >= prepare_deadline && !self.context.transaction_request_sent {
                    self.initiate_proposal(timestamp)?;
                    return Ok(());
                }
                if timestamp < primary_timeout {
                    return Ok(());
                }
            } else if timestamp < primary_timeout {
                return Ok(());
            }
        }

        if self.context.is_timed_out(timestamp) {
            if self
                .context
                .change_view_retry_at
                .is_some_and(|retry_at| timestamp < retry_at)
            {
                return Ok(());
            }

            let reason = if self.context.has_missing_proposed_transactions() {
                ChangeViewReason::TxNotFound
            } else {
                ChangeViewReason::Timeout
            };
            self.request_change_view(reason, timestamp)?;
        }

        Ok(())
    }
}

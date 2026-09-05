use super::ConsensusService;
use crate::messages::ConsensusPayload;
use crate::{ChangeViewReason, ConsensusError, ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
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
        if self.context.my_index.is_none() {
            return Err(ConsensusError::NotValidator);
        }

        info!(block_index, "Starting consensus");
        self.context.reset_for_new_block(block_index, timestamp);
        self.context.prev_hash = prev_hash;
        self.context.version = version;
        self.running = true;

        // Primary proposals are dispatched by the initial timer. This preserves
        // the C# delay after the previous block was persisted.
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
        if self.context.my_index.is_none() {
            return Err(ConsensusError::NotValidator);
        }

        self.context.prev_hash = prev_hash;
        self.context.version = version;
        let timeout = self.context.get_timeout();
        self.context.change_timer(timestamp, timeout);
        self.context.state = if self.context.is_primary() {
            crate::context::ConsensusState::Primary
        } else {
            crate::context::ConsensusState::Backup
        };
        self.running = true;

        if self.context.is_primary()
            && !self.context.prepare_request_received
            && self.context.proposal_requested()
        {
            self.initiate_proposal(timestamp)?;
        } else {
            self.check_prepare_responses()?;
            self.check_commits()?;
        }

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

        // Validate view number (ChangeView and Recovery messages can be for other views).
        #[allow(clippy::collapsible_if)]
        if !matches!(
            payload.message_type,
            ConsensusMessageType::ChangeView
                | ConsensusMessageType::RecoveryRequest
                | ConsensusMessageType::RecoveryMessage
        ) && payload.view_number != self.context.view_number
        {
            // Off-view messages are not accepted by the current round. Leave
            // them unmarked so a retransmission can be handled after a view
            // transition (RecoveryMessage is the explicit cross-view path).
            return Ok(());
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

        // Record liveness only after routing and handler validation succeed.
        self.context
            .update_last_seen_message(payload.validator_index, payload.block_index);
        // Cache only messages that passed routing and handler validation.
        self.context.mark_message_seen(&msg_hash);
        Ok(())
    }

    /// Handles timer tick for timeout detection.
    pub fn on_timer_tick(&mut self, timestamp: u64) -> ConsensusResult<()> {
        self.on_timer_tick_with_reason(timestamp, ChangeViewReason::Timeout)
    }

    /// Handles timer tick while preserving a node-level transaction timeout reason.
    pub fn on_timer_tick_with_reason(
        &mut self,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> ConsensusResult<()> {
        if !self.running {
            return Ok(());
        }

        if self.context.is_timed_out(timestamp) {
            if self.context.is_primary()
                && !self.context.proposal_requested()
                && !self.context.prepare_request_received
            {
                self.context.mark_proposal_requested();
                self.context
                    .change_timer(timestamp, self.context.prepare_request_timeout());
                self.initiate_proposal(timestamp)?;
                return Ok(());
            }

            let commit_sent = self
                .context
                .my_index
                .and_then(|index| self.context.commits.get(&index))
                .is_some();
            if commit_sent {
                // C# resends a RecoveryMessage after CommitSent instead of
                // initiating another view change, then gives the network 2T.
                let recovery = self.build_recovery_message()?;
                let payload = self
                    .create_payload(ConsensusMessageType::RecoveryMessage, recovery.serialize())?;
                self.broadcast(payload)?;
                let two_block_times = self.context.expected_block_time.saturating_mul(2);
                self.context.change_timer(timestamp, two_block_times);
            } else {
                self.request_change_view(reason, timestamp)?;
            }
        }

        Ok(())
    }
}

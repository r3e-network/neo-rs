//! Consensus view and block lifecycle resets.
//!
//! These methods preserve the exact dBFT round reset semantics used by the
//! service layer: a view reset clears proposal/preparation state while keeping
//! recovery `ChangeView` payloads, and a block reset clears all round-local
//! caches while seeding liveness tracking for the new height.

use neo_primitives::{UInt160, UInt256};

use super::{ConsensusContext, ConsensusState};

impl ConsensusContext {
    /// Resets the context for a new view.
    pub fn reset_for_new_view(&mut self, new_view: u8, timestamp: u64) {
        self.view_number = new_view;
        self.view_start_time = timestamp;
        self.timer_extension = 0;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear proposal data.
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.available_tx_hashes.clear();
        self.available_tx_metrics.clear();
        self.nonce = 0;

        // Clear signatures.
        self.prepare_request_received = false;
        self.transaction_request_sent = false;
        self.transaction_request_sent_at = None;
        self.commit_recovery_sent_at = None;
        self.change_view_retry_at = None;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.prepare_request_invocation = None;
        // Keep change_views for recovery.
    }

    /// Resets the context for a new block.
    pub fn reset_for_new_block(&mut self, block_index: u32, timestamp: u64) {
        self.block_index = block_index;
        self.view_number = 0;
        self.view_start_time = timestamp;
        self.timer_extension = 0;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear all data.
        self.version = 0;
        self.prev_hash = UInt256::zero();
        self.previous_block_timestamp = 0;
        self.next_consensus = UInt160::zero();
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.available_tx_hashes.clear();
        self.available_tx_metrics.clear();
        self.nonce = 0;
        self.prepare_request_received = false;
        self.transaction_request_sent = false;
        self.transaction_request_sent_at = None;
        self.commit_recovery_sent_at = None;
        self.change_view_retry_at = None;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.commits.clear();
        self.commit_view_numbers.clear();
        self.change_views.clear();
        self.invalid_transactions.clear();
        self.prepare_request_invocation = None;
        self.change_view_invocations.clear();
        self.commit_invocations.clear();
        self.last_change_view_timestamps.clear();
        let previous_last_seen_messages = std::mem::take(&mut self.last_seen_messages);
        let previous_height = block_index.saturating_sub(1);
        for validator in &self.validators {
            let last_seen_message = previous_last_seen_messages
                .get(&validator.index)
                .copied()
                .unwrap_or(previous_height);
            self.last_seen_messages
                .insert(validator.index, last_seen_message);
        }
        if let Some(my_index) = self.my_index {
            self.last_seen_messages.insert(my_index, block_index);
        }

        // Clear message hash cache to prevent memory growth.
        self.seen_message_hashes.clear();
    }
}

//! Consensus context construction defaults.
//!
//! The root module owns the public state shape. This module owns the initial
//! field values for a fresh dBFT round so the facade does not grow constructor
//! mechanics.

use std::collections::{HashMap, HashSet};

use neo_primitives::{UInt160, UInt256};

use super::{
    ConsensusContext, ConsensusState, DEFAULT_MAX_BLOCK_SIZE, DEFAULT_MAX_BLOCK_SYSTEM_FEE,
    ValidatorInfo,
};

impl ConsensusContext {
    /// Creates a new consensus context.
    ///
    /// `block_time_ms` sets the expected block interval. Pass `None` (or `0`)
    /// to fall back to [`DEFAULT_BLOCK_TIME_MS`](super::DEFAULT_BLOCK_TIME_MS).
    #[must_use]
    pub fn new(
        block_index: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        block_time_ms: Option<u64>,
    ) -> Self {
        Self {
            block_index,
            view_number: 0,
            validators,
            my_index,
            state: ConsensusState::Initial,
            view_start_time: 0,
            timer_extension: 0,
            expected_block_time: super::policy::effective_block_time(block_time_ms),
            version: 0,
            prev_hash: UInt256::zero(),
            previous_block_timestamp: 0,
            next_consensus: UInt160::zero(),
            proposed_block_hash: None,
            preparation_hash: None,
            proposed_timestamp: 0,
            proposed_tx_hashes: Vec::new(),
            available_tx_hashes: HashSet::new(),
            available_tx_metrics: HashMap::new(),
            max_block_size: DEFAULT_MAX_BLOCK_SIZE,
            max_block_system_fee: DEFAULT_MAX_BLOCK_SYSTEM_FEE,
            nonce: 0,
            prepare_request_received: false,
            transaction_request_sent: false,
            transaction_request_sent_at: None,
            commit_recovery_sent_at: None,
            change_view_retry_at: None,
            prepare_responses: HashMap::new(),
            prepare_response_hashes: HashMap::new(),
            commits: HashMap::new(),
            commit_view_numbers: HashMap::new(),
            change_views: HashMap::new(),
            invalid_transactions: HashMap::new(),
            prepare_request_invocation: None,
            change_view_invocations: HashMap::new(),
            commit_invocations: HashMap::new(),
            last_change_view_timestamps: HashMap::new(),
            last_seen_messages: HashMap::new(),
            seen_message_hashes: Self::new_seen_message_cache(),
        }
    }
}

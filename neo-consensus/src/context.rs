//! Consensus context - tracks the current consensus state.

use crate::{ChangeViewReason, ConsensusError, ConsensusResult};
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use std::collections::HashMap;

/// Block time in milliseconds (15 seconds for Neo N3)
pub const BLOCK_TIME_MS: u64 = 15_000;

/// Maximum validators in dBFT
pub const MAX_VALIDATORS: usize = 21;

/// Consensus state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConsensusState {
    /// Initial state, waiting to start
    #[default]
    Initial,
    /// Primary (speaker) mode - proposing blocks
    Primary,
    /// Backup (validator) mode - validating proposals
    Backup,
    /// View changing - requesting view change
    ViewChanging,
    /// Committed - block has been committed
    Committed,
}

/// Validator information
#[derive(Debug, Clone)]
pub struct ValidatorInfo {
    /// Validator index (0 to n-1)
    pub index: u8,
    /// Public key
    pub public_key: ECPoint,
    /// Script hash (account)
    pub script_hash: UInt160,
}

/// Consensus context holding all state for the current consensus round
#[derive(Debug)]
pub struct ConsensusContext {
    /// Current block index being proposed
    pub block_index: u32,
    /// Current view number (increments on view change)
    pub view_number: u8,
    /// List of validators for this round
    pub validators: Vec<ValidatorInfo>,
    /// My validator index (None if not a validator)
    pub my_index: Option<u8>,
    /// Current consensus state
    pub state: ConsensusState,
    /// Timestamp when this view started
    pub view_start_time: u64,
    /// Expected block time
    pub expected_block_time: u64,

    // Proposal data
    /// Proposed block hash (from PrepareRequest)
    pub proposed_block_hash: Option<UInt256>,
    /// Proposed block timestamp
    pub proposed_timestamp: u64,
    /// Proposed transaction hashes
    pub proposed_tx_hashes: Vec<UInt256>,
    /// Nonce for the block
    pub nonce: u64,

    // Signature tracking
    /// PrepareRequest received from primary
    pub prepare_request_received: bool,
    /// PrepareResponse signatures (validator_index -> signature)
    pub prepare_responses: HashMap<u8, Vec<u8>>,
    /// Commit signatures (validator_index -> signature)
    pub commits: HashMap<u8, Vec<u8>>,
    /// ChangeView requests (validator_index -> (new_view, reason))
    pub change_views: HashMap<u8, (u8, ChangeViewReason)>,

    // Recovery
    /// Last change view timestamp per validator
    pub last_change_view_timestamps: HashMap<u8, u64>,
}

impl ConsensusContext {
    /// Creates a new consensus context
    pub fn new(block_index: u32, validators: Vec<ValidatorInfo>, my_index: Option<u8>) -> Self {
        Self {
            block_index,
            view_number: 0,
            validators,
            my_index,
            state: ConsensusState::Initial,
            view_start_time: 0,
            expected_block_time: 0,
            proposed_block_hash: None,
            proposed_timestamp: 0,
            proposed_tx_hashes: Vec::new(),
            nonce: 0,
            prepare_request_received: false,
            prepare_responses: HashMap::new(),
            commits: HashMap::new(),
            change_views: HashMap::new(),
            last_change_view_timestamps: HashMap::new(),
        }
    }

    /// Returns the number of validators
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Returns the number of faulty nodes tolerated: f = (n-1)/3
    pub fn f(&self) -> usize {
        (self.validator_count() - 1) / 3
    }

    /// Returns the number of nodes required for consensus: M = n - f
    pub fn m(&self) -> usize {
        self.validator_count() - self.f()
    }

    /// Returns the primary (speaker) index for the current view
    pub fn primary_index(&self) -> u8 {
        let p = (self.block_index as usize + self.view_number as usize) % self.validator_count();
        p as u8
    }

    /// Returns true if this node is the primary for the current view
    pub fn is_primary(&self) -> bool {
        self.my_index == Some(self.primary_index())
    }

    /// Returns true if this node is a backup (non-primary validator)
    pub fn is_backup(&self) -> bool {
        match self.my_index {
            Some(idx) => idx != self.primary_index(),
            None => false,
        }
    }

    /// Returns true if we have enough prepare responses (M signatures)
    pub fn has_enough_prepare_responses(&self) -> bool {
        // Count: primary's implicit response + explicit responses
        let count = if self.prepare_request_received { 1 } else { 0 } + self.prepare_responses.len();
        count >= self.m()
    }

    /// Returns true if we have enough commits (M signatures)
    pub fn has_enough_commits(&self) -> bool {
        self.commits.len() >= self.m()
    }

    /// Returns true if we have enough change view requests (M requests)
    pub fn has_enough_change_views(&self, new_view: u8) -> bool {
        let count = self
            .change_views
            .values()
            .filter(|(v, _)| *v == new_view)
            .count();
        count >= self.m()
    }

    /// Resets the context for a new view
    pub fn reset_for_new_view(&mut self, new_view: u8, timestamp: u64) {
        self.view_number = new_view;
        self.view_start_time = timestamp;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear proposal data
        self.proposed_block_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.nonce = 0;

        // Clear signatures
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.commits.clear();
        // Keep change_views for recovery
    }

    /// Resets the context for a new block
    pub fn reset_for_new_block(&mut self, block_index: u32, timestamp: u64) {
        self.block_index = block_index;
        self.view_number = 0;
        self.view_start_time = timestamp;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear all data
        self.proposed_block_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.nonce = 0;
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.commits.clear();
        self.change_views.clear();
        self.last_change_view_timestamps.clear();
    }

    /// Adds a prepare response signature
    pub fn add_prepare_response(&mut self, validator_index: u8, signature: Vec<u8>) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.prepare_responses.insert(validator_index, signature);
        Ok(())
    }

    /// Adds a commit signature
    pub fn add_commit(&mut self, validator_index: u8, signature: Vec<u8>) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.commits.insert(validator_index, signature);
        Ok(())
    }

    /// Adds a change view request
    pub fn add_change_view(
        &mut self,
        validator_index: u8,
        new_view: u8,
        reason: ChangeViewReason,
        timestamp: u64,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.change_views.insert(validator_index, (new_view, reason));
        self.last_change_view_timestamps.insert(validator_index, timestamp);
        Ok(())
    }

    /// Gets the timeout duration for the current view
    pub fn get_timeout(&self) -> u64 {
        // Base timeout + exponential backoff for view changes
        BLOCK_TIME_MS << self.view_number.min(4)
    }

    /// Checks if the current view has timed out
    pub fn is_timed_out(&self, current_time: u64) -> bool {
        current_time > self.view_start_time + self.get_timeout()
    }

    /// Collects all commit signatures for block finalization
    pub fn collect_commit_signatures(&self) -> Vec<(u8, Vec<u8>)> {
        self.commits
            .iter()
            .map(|(idx, sig)| (*idx, sig.clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
        (0..count)
            .map(|i| ValidatorInfo {
                index: i as u8,
                public_key: ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
                script_hash: UInt160::zero(),
            })
            .collect()
    }

    #[test]
    fn test_consensus_context_new() {
        let validators = create_test_validators(7);
        let ctx = ConsensusContext::new(100, validators, Some(0));

        assert_eq!(ctx.block_index, 100);
        assert_eq!(ctx.view_number, 0);
        assert_eq!(ctx.validator_count(), 7);
        assert_eq!(ctx.my_index, Some(0));
    }

    #[test]
    fn test_f_and_m_calculations() {
        // 7 validators: f = 2, M = 5
        let validators = create_test_validators(7);
        let ctx = ConsensusContext::new(0, validators, None);
        assert_eq!(ctx.f(), 2);
        assert_eq!(ctx.m(), 5);

        // 4 validators: f = 1, M = 3
        let validators = create_test_validators(4);
        let ctx = ConsensusContext::new(0, validators, None);
        assert_eq!(ctx.f(), 1);
        assert_eq!(ctx.m(), 3);

        // 21 validators: f = 6, M = 15
        let validators = create_test_validators(21);
        let ctx = ConsensusContext::new(0, validators, None);
        assert_eq!(ctx.f(), 6);
        assert_eq!(ctx.m(), 15);
    }

    #[test]
    fn test_primary_index() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(0, validators, Some(0));

        // Block 0, view 0: primary = 0
        assert_eq!(ctx.primary_index(), 0);
        assert!(ctx.is_primary());

        // Block 0, view 1: primary = 1
        ctx.view_number = 1;
        assert_eq!(ctx.primary_index(), 1);
        assert!(!ctx.is_primary());

        // Block 7, view 0: primary = 0 (7 % 7 = 0)
        ctx.block_index = 7;
        ctx.view_number = 0;
        assert_eq!(ctx.primary_index(), 0);
    }

    #[test]
    fn test_has_enough_responses() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(0, validators, Some(0));

        // Need M = 5 responses
        assert!(!ctx.has_enough_prepare_responses());

        ctx.prepare_request_received = true;
        ctx.prepare_responses.insert(1, vec![1]);
        ctx.prepare_responses.insert(2, vec![2]);
        ctx.prepare_responses.insert(3, vec![3]);
        assert!(!ctx.has_enough_prepare_responses()); // 4 < 5

        ctx.prepare_responses.insert(4, vec![4]);
        assert!(ctx.has_enough_prepare_responses()); // 5 >= 5
    }

    #[test]
    fn test_reset_for_new_view() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(0, validators, Some(1));

        ctx.prepare_request_received = true;
        ctx.prepare_responses.insert(0, vec![0]);
        ctx.commits.insert(0, vec![0]);

        ctx.reset_for_new_view(1, 1000);

        assert_eq!(ctx.view_number, 1);
        assert_eq!(ctx.view_start_time, 1000);
        assert!(!ctx.prepare_request_received);
        assert!(ctx.prepare_responses.is_empty());
        assert!(ctx.commits.is_empty());
    }

    #[test]
    fn test_timeout_calculation() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(0, validators, None);

        // View 0: 15s
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS);

        // View 1: 30s
        ctx.view_number = 1;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 2);

        // View 2: 60s
        ctx.view_number = 2;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 4);

        // View 4+: capped at 240s
        ctx.view_number = 10;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 16);
    }
}

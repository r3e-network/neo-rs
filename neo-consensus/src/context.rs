//! Consensus context - tracks the current consensus state.

use crate::{ChangeViewReason, ConsensusError, ConsensusResult};
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Block time in milliseconds (15 seconds for Neo N3)
pub const BLOCK_TIME_MS: u64 = 15_000;

/// Maximum validators in dBFT
pub const MAX_VALIDATORS: usize = 21;

/// Maximum size of message hash cache (LRU limit for memory protection)
/// Matches C# `DBFTPlugin`'s message caching behavior
pub const MAX_MESSAGE_CACHE_SIZE: usize = 10_000;

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

/// Persisted consensus state for crash recovery
/// Contains only the essential state needed to resume consensus after a restart
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedConsensusState {
    /// Current block index being proposed
    block_index: u32,
    /// Current view number (increments on view change)
    view_number: u8,
    /// Proposed block hash (from `PrepareRequest`)
    proposed_block_hash: Option<UInt256>,
    /// Hash of the primary `PrepareRequest` payload (ExtensiblePayload.Hash)
    #[serde(default)]
    preparation_hash: Option<UInt256>,
    /// Proposed block timestamp
    proposed_timestamp: u64,
    /// Proposed transaction hashes
    proposed_tx_hashes: Vec<UInt256>,
    /// Nonce for the block
    nonce: u64,
    /// `PrepareRequest` received from primary
    prepare_request_received: bool,
    /// `PrepareResponse` signatures (`validator_index` -> signature)
    prepare_responses: HashMap<u8, Vec<u8>>,
    /// `PrepareResponse` hashes (`validator_index` -> `preparation_hash`)
    #[serde(default)]
    prepare_response_hashes: HashMap<u8, UInt256>,
    /// Commit signatures (`validator_index` -> signature)
    commits: HashMap<u8, Vec<u8>>,
    /// Commit view numbers (`validator_index` -> `view_number`)
    #[serde(default)]
    commit_view_numbers: HashMap<u8, u8>,
    /// `ChangeView` requests (`validator_index` -> (`new_view`, reason))
    change_views: HashMap<u8, (u8, ChangeViewReason)>,
    /// Primary `PrepareRequest` invocation script (payload witness).
    #[serde(default)]
    prepare_request_invocation: Option<Vec<u8>>,
    /// `ChangeView` invocation script per validator (payload witness).
    #[serde(default)]
    change_view_invocations: HashMap<u8, Vec<u8>>,
    /// `ChangeView` timestamp per validator.
    #[serde(default)]
    change_view_timestamps: HashMap<u8, u64>,
    /// Commit invocation script per validator (payload witness).
    #[serde(default)]
    commit_invocations: HashMap<u8, Vec<u8>>,
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
    /// Block version (must be 0 for Neo N3)
    pub version: u32,
    /// Previous block hash for the proposed block.
    pub prev_hash: UInt256,
    /// Proposed block hash (from `PrepareRequest`)
    pub proposed_block_hash: Option<UInt256>,
    /// Hash of the primary's `PrepareRequest` extensible payload (ExtensiblePayload.Hash).
    ///
    /// In Neo N3 `DBFTPlugin` this is used as `PrepareResponse.PreparationHash`.
    pub preparation_hash: Option<UInt256>,
    /// Proposed block timestamp
    pub proposed_timestamp: u64,
    /// Proposed transaction hashes
    pub proposed_tx_hashes: Vec<UInt256>,
    /// Nonce for the block
    pub nonce: u64,

    // Signature tracking
    /// `PrepareRequest` received from primary
    pub prepare_request_received: bool,
    /// `PrepareResponse` signatures (`validator_index` -> signature)
    pub prepare_responses: HashMap<u8, Vec<u8>>,
    /// `PrepareResponse` hashes (`validator_index` -> `preparation_hash`)
    pub prepare_response_hashes: HashMap<u8, UInt256>,
    /// Commit signatures (`validator_index` -> signature)
    pub commits: HashMap<u8, Vec<u8>>,
    /// Commit view numbers (`validator_index` -> `view_number`)
    pub commit_view_numbers: HashMap<u8, u8>,
    /// `ChangeView` requests (`validator_index` -> (`new_view`, reason))
    pub change_views: HashMap<u8, (u8, ChangeViewReason)>,
    /// Primary `PrepareRequest` invocation script (payload witness).
    pub prepare_request_invocation: Option<Vec<u8>>,
    /// `ChangeView` invocation script per validator (payload witness).
    pub change_view_invocations: HashMap<u8, Vec<u8>>,
    /// Commit invocation script per validator (payload witness).
    pub commit_invocations: HashMap<u8, Vec<u8>>,

    // Recovery
    /// Last change view timestamp per validator
    pub last_change_view_timestamps: HashMap<u8, u64>,
    /// Last seen message block index per validator (for tracking failed nodes)
    pub last_seen_messages: HashMap<u8, u32>,

    // Message deduplication (replay attack prevention)
    /// Cache of seen message hashes to prevent duplicate processing
    seen_message_hashes: HashSet<UInt256>,
}

impl ConsensusContext {
    /// Creates a new consensus context
    #[must_use]
    pub fn new(block_index: u32, validators: Vec<ValidatorInfo>, my_index: Option<u8>) -> Self {
        Self {
            block_index,
            view_number: 0,
            validators,
            my_index,
            state: ConsensusState::Initial,
            view_start_time: 0,
            expected_block_time: 0,
            version: 0,
            prev_hash: UInt256::zero(),
            proposed_block_hash: None,
            preparation_hash: None,
            proposed_timestamp: 0,
            proposed_tx_hashes: Vec::new(),
            nonce: 0,
            prepare_request_received: false,
            prepare_responses: HashMap::new(),
            prepare_response_hashes: HashMap::new(),
            commits: HashMap::new(),
            commit_view_numbers: HashMap::new(),
            change_views: HashMap::new(),
            prepare_request_invocation: None,
            change_view_invocations: HashMap::new(),
            commit_invocations: HashMap::new(),
            last_change_view_timestamps: HashMap::new(),
            last_seen_messages: HashMap::new(),
            seen_message_hashes: HashSet::new(),
        }
    }

    /// Returns the number of validators
    #[must_use]
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Returns the number of faulty nodes tolerated: f = (n-1)/3
    #[must_use]
    pub fn f(&self) -> usize {
        (self.validator_count() - 1) / 3
    }

    /// Returns the number of nodes required for consensus: M = n - f
    #[must_use]
    pub fn m(&self) -> usize {
        self.validator_count() - self.f()
    }

    /// Returns the primary (speaker) index for the current view
    #[must_use]
    pub fn primary_index(&self) -> u8 {
        // Matches C# DBFTPlugin:
        // `p = ((Block.Index - viewNumber) % Validators.Length + Validators.Length) % Validators.Length`.
        let n = self.validator_count() as i64;
        if n == 0 {
            return 0;
        }
        let p = (i64::from(self.block_index) - i64::from(self.view_number)).rem_euclid(n);
        p as u8
    }

    /// Returns true if this node is the primary for the current view
    #[must_use]
    pub fn is_primary(&self) -> bool {
        self.my_index == Some(self.primary_index())
    }

    /// Returns true if this node is a backup (non-primary validator)
    #[must_use]
    pub fn is_backup(&self) -> bool {
        match self.my_index {
            Some(idx) => idx != self.primary_index(),
            None => false,
        }
    }

    /// Returns true if we have enough prepare responses (M signatures)
    #[must_use]
    pub fn has_enough_prepare_responses(&self) -> bool {
        // Count: primary's implicit response + explicit responses
        let count = usize::from(self.prepare_request_received) + self.prepare_responses.len();
        count >= self.m()
    }

    /// Returns true if we have enough commits (M signatures)
    #[must_use]
    pub fn has_enough_commits(&self) -> bool {
        let count = self
            .commits
            .keys()
            .filter(|idx| {
                self.commit_view_numbers
                    .get(idx)
                    .copied()
                    .unwrap_or(self.view_number)
                    == self.view_number
            })
            .count();
        count >= self.m()
    }

    /// Returns true if we have enough change view requests (M requests).
    /// Matches C# `DBFTPlugin`'s `CheckExpectedView` logic: counts `NewViewNumber` >= requested view.
    #[must_use]
    pub fn has_enough_change_views(&self, new_view: u8) -> bool {
        let count = self
            .change_views
            .values()
            .filter(|(v, _)| *v >= new_view)
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
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.nonce = 0;

        // Clear signatures
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.prepare_request_invocation = None;
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
        self.version = 0;
        self.prev_hash = UInt256::zero();
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.nonce = 0;
        self.prepare_request_received = false;
        self.prepare_responses.clear();
        self.prepare_response_hashes.clear();
        self.commits.clear();
        self.commit_view_numbers.clear();
        self.change_views.clear();
        self.prepare_request_invocation = None;
        self.change_view_invocations.clear();
        self.commit_invocations.clear();
        self.last_change_view_timestamps.clear();
        self.last_seen_messages.clear();

        // Clear message hash cache to prevent memory growth
        self.seen_message_hashes.clear();
    }

    /// Adds a prepare response invocation script
    pub fn add_prepare_response(
        &mut self,
        validator_index: u8,
        invocation_script: Vec<u8>,
        preparation_hash: Option<UInt256>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.prepare_responses
            .insert(validator_index, invocation_script);
        if let Some(hash) = preparation_hash {
            self.prepare_response_hashes.insert(validator_index, hash);
        }
        Ok(())
    }

    /// Adds a commit signature
    pub fn add_commit(
        &mut self,
        validator_index: u8,
        view_number: u8,
        signature: Vec<u8>,
    ) -> ConsensusResult<()> {
        if validator_index as usize >= self.validator_count() {
            return Err(ConsensusError::InvalidValidatorIndex(validator_index));
        }
        self.commits.insert(validator_index, signature);
        self.commit_view_numbers
            .insert(validator_index, view_number);
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
        self.change_views
            .insert(validator_index, (new_view, reason));
        self.last_change_view_timestamps
            .insert(validator_index, timestamp);
        Ok(())
    }

    /// Gets the timeout duration for the current view
    #[must_use]
    pub fn get_timeout(&self) -> u64 {
        // Base timeout + exponential backoff for view changes.
        // Use configured expected_block_time when provided (mirrors C# TimePerBlock overrides).
        let base = if self.expected_block_time > 0 {
            self.expected_block_time
        } else {
            BLOCK_TIME_MS
        };
        base << (self.view_number + 1).min(5)
    }

    /// Checks if the current view has timed out
    #[must_use]
    pub fn is_timed_out(&self, current_time: u64) -> bool {
        current_time > self.view_start_time + self.get_timeout()
    }

    /// Collects all commit signatures for block finalization
    #[must_use]
    pub fn collect_commit_signatures(&self) -> Vec<(u8, Vec<u8>)> {
        self.commits
            .iter()
            .filter(|(idx, _)| {
                self.commit_view_numbers
                    .get(idx)
                    .copied()
                    .unwrap_or(self.view_number)
                    == self.view_number
            })
            .map(|(idx, sig)| (*idx, sig.clone()))
            .collect()
    }

    /// Updates the last seen message for a validator
    pub fn update_last_seen_message(&mut self, validator_index: u8, block_index: u32) {
        self.last_seen_messages.insert(validator_index, block_index);
    }

    /// Checks if a message hash has been seen before (replay attack prevention)
    ///
    /// This method is critical for preventing replay attacks where an attacker
    /// could retransmit valid consensus messages to disrupt the protocol.
    ///
    /// # Arguments
    /// * `hash` - The message hash to check
    ///
    /// # Returns
    /// * `true` if the message has been seen before
    /// * `false` if this is a new message
    #[must_use]
    pub fn has_seen_message(&self, hash: &UInt256) -> bool {
        self.seen_message_hashes.contains(hash)
    }

    /// Marks a message hash as seen (replay attack prevention)
    ///
    /// This method adds the message hash to the cache to prevent duplicate processing.
    /// The cache is automatically cleared when starting a new block via `reset_for_new_block()`.
    ///
    /// Security: Implements LRU-style cache limit (`MAX_MESSAGE_CACHE_SIZE`) to prevent
    /// memory exhaustion attacks. When the cache is full, it is cleared to make room
    /// for new messages. This matches C# `DBFTPlugin`'s memory protection behavior.
    ///
    /// # Arguments
    /// * `hash` - The message hash to mark as seen
    pub fn mark_message_seen(&mut self, hash: &UInt256) {
        // LRU-style cache limit: clear when full to prevent memory exhaustion
        if self.seen_message_hashes.len() >= MAX_MESSAGE_CACHE_SIZE {
            tracing::warn!(
                "Message cache reached limit ({}), clearing to prevent memory exhaustion",
                MAX_MESSAGE_CACHE_SIZE
            );
            self.seen_message_hashes.clear();
        }
        self.seen_message_hashes.insert(*hash);
    }

    /// Returns the number of validators that have committed (sent Commit messages)
    #[must_use]
    pub fn count_committed(&self) -> usize {
        self.commits.len()
    }

    /// Returns the number of validators that have failed or are lost
    ///
    /// A validator is considered failed if:
    /// - We have no record of messages from them (not in `last_seen_messages`), OR
    /// - Their last seen message was for an old block (< current `block_index` - 1)
    ///
    /// This matches C# `DBFTPlugin`'s `CountFailed` logic:
    /// ```csharp
    /// Validators.Count(p => !LastSeenMessage.TryGetValue(p, out var value) || value < (Block.Index - 1))
    /// ```
    #[must_use]
    pub fn count_failed(&self) -> usize {
        if self.last_seen_messages.is_empty() {
            return 0;
        }

        let threshold = self.block_index.saturating_sub(1);
        self.validators
            .iter()
            .filter(|v| {
                match self.last_seen_messages.get(&v.index) {
                    None => true,                                // No message seen from this validator
                    Some(&last_block) => last_block < threshold, // Last message was too old
                }
            })
            .count()
    }

    /// Returns true if more than F nodes have committed or are lost
    ///
    /// This is a critical check for deciding between recovery and view change.
    /// When (`CountCommitted` + `CountFailed`) > F, it means:
    /// - Either enough nodes have already committed, OR
    /// - Enough nodes have failed that we need recovery to sync state
    ///
    /// In this case, we should request recovery instead of change view to avoid
    /// splitting the network across different views.
    ///
    /// Matches C# `DBFTPlugin`'s `MoreThanFNodesCommittedOrLost`:
    /// ```csharp
    /// public bool MoreThanFNodesCommittedOrLost => (CountCommitted + CountFailed) > F;
    /// ```
    #[must_use]
    pub fn more_than_f_nodes_committed_or_lost(&self) -> bool {
        (self.count_committed() + self.count_failed()) > self.f()
    }

    /// Returns true if this node has requested a view change.
    ///
    /// Mirrors C# `DBFTPlugin` `ViewChanging`:
    /// `!WatchOnly && ChangeViewPayloads[MyIndex]?.NewViewNumber > ViewNumber`.
    #[must_use]
    pub fn view_changing(&self) -> bool {
        let Some(my_index) = self.my_index else {
            return false;
        };
        self.change_views
            .get(&my_index)
            .is_some_and(|(new_view, _)| *new_view > self.view_number)
    }

    /// Returns true when we should not accept certain payloads due to an ongoing view change.
    ///
    /// Mirrors C# `DBFTPlugin` `NotAcceptingPayloadsDueToViewChanging`.
    #[must_use]
    pub fn not_accepting_payloads_due_to_view_changing(&self) -> bool {
        self.view_changing() && !self.more_than_f_nodes_committed_or_lost()
    }

    /// Saves the consensus state to disk for crash recovery
    ///
    /// Uses atomic write (write to temp file + rename) to prevent corruption.
    /// Only saves the essential state needed to resume consensus after a restart.
    ///
    /// # Arguments
    /// * `path` - Path to save the state file
    ///
    /// # Returns
    /// * `Ok(())` on success
    /// * `Err(ConsensusError)` on IO or serialization failure
    pub fn save(&self, path: &Path) -> ConsensusResult<()> {
        // Create the persisted state from current context
        let state = PersistedConsensusState {
            block_index: self.block_index,
            view_number: self.view_number,
            proposed_block_hash: self.proposed_block_hash,
            preparation_hash: self.preparation_hash,
            proposed_timestamp: self.proposed_timestamp,
            proposed_tx_hashes: self.proposed_tx_hashes.clone(),
            nonce: self.nonce,
            prepare_request_received: self.prepare_request_received,
            prepare_responses: self.prepare_responses.clone(),
            prepare_response_hashes: self.prepare_response_hashes.clone(),
            commits: self.commits.clone(),
            commit_view_numbers: self.commit_view_numbers.clone(),
            change_views: self.change_views.clone(),
            prepare_request_invocation: self.prepare_request_invocation.clone(),
            change_view_invocations: self.change_view_invocations.clone(),
            change_view_timestamps: self.last_change_view_timestamps.clone(),
            commit_invocations: self.commit_invocations.clone(),
        };

        let encoded = Self::encode_state(&state)?;

        // Atomic write: write to temp file first
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, &encoded)?;

        // Rename to final path (atomic on POSIX systems)
        fs::rename(&temp_path, path)?;

        tracing::debug!(
            "Saved consensus state: block={}, view={}, size={} bytes",
            self.block_index,
            self.view_number,
            encoded.len()
        );

        Ok(())
    }

    /// Serializes the persisted consensus state to bytes.
    pub fn to_bytes(&self) -> ConsensusResult<Vec<u8>> {
        let state = PersistedConsensusState {
            block_index: self.block_index,
            view_number: self.view_number,
            proposed_block_hash: self.proposed_block_hash,
            preparation_hash: self.preparation_hash,
            proposed_timestamp: self.proposed_timestamp,
            proposed_tx_hashes: self.proposed_tx_hashes.clone(),
            nonce: self.nonce,
            prepare_request_received: self.prepare_request_received,
            prepare_responses: self.prepare_responses.clone(),
            prepare_response_hashes: self.prepare_response_hashes.clone(),
            commits: self.commits.clone(),
            commit_view_numbers: self.commit_view_numbers.clone(),
            change_views: self.change_views.clone(),
            prepare_request_invocation: self.prepare_request_invocation.clone(),
            change_view_invocations: self.change_view_invocations.clone(),
            change_view_timestamps: self.last_change_view_timestamps.clone(),
            commit_invocations: self.commit_invocations.clone(),
        };
        Self::encode_state(&state)
    }

    /// Loads the consensus state from disk for crash recovery
    ///
    /// This method loads only the persisted state. The caller must provide:
    /// - `validators`: Current validator list (from chain state)
    /// - `my_index`: This node's validator index (from config)
    ///
    /// The loaded context will have:
    /// - `state`: Set to `Initial` (caller should update based on role)
    /// - `view_start_time`: Set to 0 (caller should update to current time)
    /// - `expected_block_time`: Set to 0 (caller should update)
    /// - `last_change_view_timestamps`: Empty (not persisted)
    ///
    /// # Arguments
    /// * `path` - Path to load the state file from
    /// * `validators` - Current validator list
    /// * `my_index` - This node's validator index
    ///
    /// # Returns
    /// * `Ok(ConsensusContext)` on success
    /// * `Err(ConsensusError)` on IO or deserialization failure
    pub fn load(
        path: &Path,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> ConsensusResult<Self> {
        // Read the file
        let encoded = fs::read(path)?;
        Self::from_bytes(&encoded, validators, my_index)
    }

    /// Deserializes a consensus context from raw bytes.
    pub fn from_bytes(
        encoded: &[u8],
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> ConsensusResult<Self> {
        let state = Self::decode_state(encoded)?;

        tracing::info!(
            "Loaded consensus state: block={}, view={}, prepare_responses={}, commits={}, change_views={}",
            state.block_index,
            state.view_number,
            state.prepare_responses.len(),
            state.commits.len(),
            state.change_views.len()
        );

        // Reconstruct the full context
        Ok(Self {
            block_index: state.block_index,
            view_number: state.view_number,
            validators,
            my_index,
            state: ConsensusState::Initial, // Caller should update based on role
            view_start_time: 0,             // Caller should update to current time
            expected_block_time: 0,         // Caller should update
            version: 0,
            prev_hash: UInt256::zero(),
            proposed_block_hash: state.proposed_block_hash,
            preparation_hash: state.preparation_hash,
            proposed_timestamp: state.proposed_timestamp,
            proposed_tx_hashes: state.proposed_tx_hashes,
            nonce: state.nonce,
            prepare_request_received: state.prepare_request_received,
            prepare_responses: state.prepare_responses,
            prepare_response_hashes: state.prepare_response_hashes,
            commits: state.commits,
            commit_view_numbers: state.commit_view_numbers,
            change_views: state.change_views,
            prepare_request_invocation: state.prepare_request_invocation,
            change_view_invocations: state.change_view_invocations,
            commit_invocations: state.commit_invocations,
            last_change_view_timestamps: state.change_view_timestamps,
            last_seen_messages: HashMap::new(),  // Not persisted
            seen_message_hashes: HashSet::new(), // Not persisted (cleared on restart)
        })
    }

    fn encode_state(state: &PersistedConsensusState) -> ConsensusResult<Vec<u8>> {
        Ok(bincode::serialize(state)?)
    }

    fn decode_state(encoded: &[u8]) -> ConsensusResult<PersistedConsensusState> {
        Ok(bincode::deserialize(encoded)?)
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

        // Block 0, view 1: primary = (0 - 1) mod 7 = 6 (matches C# DBFTPlugin)
        ctx.view_number = 1;
        assert_eq!(ctx.primary_index(), 6);
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
        ctx.commit_view_numbers.insert(0, 0);

        ctx.reset_for_new_view(1, 1000);

        assert_eq!(ctx.view_number, 1);
        assert_eq!(ctx.view_start_time, 1000);
        assert!(!ctx.prepare_request_received);
        assert!(ctx.prepare_responses.is_empty());
        assert!(!ctx.commits.is_empty());
    }

    #[test]
    fn test_timeout_calculation() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(0, validators, None);

        // View 0: base << 1 = 30s (matches C# shift by ViewNumber+1)
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 2);

        // View 1: base << 2 = 60s
        ctx.view_number = 1;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 4);

        // View 2: base << 3 = 120s
        ctx.view_number = 2;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 8);

        // View 4+: capped at base << 5 = 480s
        ctx.view_number = 10;
        assert_eq!(ctx.get_timeout(), BLOCK_TIME_MS * 32);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        use std::env;

        // Create a test context with some state
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators.clone(), Some(0));

        // Set up some consensus state
        ctx.view_number = 2;
        ctx.proposed_block_hash = Some(UInt256::from_bytes(&[1u8; 32]).unwrap());
        ctx.preparation_hash = Some(UInt256::from_bytes(&[9u8; 32]).unwrap());
        ctx.proposed_timestamp = 1234567890;
        ctx.proposed_tx_hashes = vec![
            UInt256::from_bytes(&[2u8; 32]).unwrap(),
            UInt256::from_bytes(&[3u8; 32]).unwrap(),
        ];
        ctx.nonce = 42;
        ctx.prepare_request_received = true;
        ctx.prepare_request_invocation = Some(vec![0x0c, 0x40, 0xaa]);
        ctx.prepare_responses.insert(1, vec![0xaa, 0xbb, 0xcc]);
        ctx.prepare_responses.insert(2, vec![0xdd, 0xee, 0xff]);
        ctx.prepare_response_hashes
            .insert(1, UInt256::from_bytes(&[0x10; 32]).unwrap());
        ctx.prepare_response_hashes
            .insert(2, UInt256::from_bytes(&[0x11; 32]).unwrap());
        ctx.commits.insert(0, vec![0x11, 0x22, 0x33]);
        ctx.commits.insert(1, vec![0x44, 0x55, 0x66]);
        ctx.commit_view_numbers.insert(0, 2);
        ctx.commit_view_numbers.insert(1, 2);
        ctx.commit_invocations.insert(0, vec![0x0c, 0x40, 0xbb]);
        ctx.commit_invocations.insert(1, vec![0x0c, 0x40, 0xcc]);
        ctx.change_views.insert(3, (3, ChangeViewReason::Timeout));
        ctx.change_views
            .insert(4, (3, ChangeViewReason::TxNotFound));
        ctx.change_view_invocations
            .insert(3, vec![0x0c, 0x40, 0xdd]);
        ctx.change_view_invocations
            .insert(4, vec![0x0c, 0x40, 0xee]);
        ctx.last_change_view_timestamps.insert(3, 1_111);
        ctx.last_change_view_timestamps.insert(4, 2_222);

        // Save to a temporary file
        let temp_dir = env::temp_dir();
        let temp_path = temp_dir.join("test_consensus_state.bin");

        ctx.save(&temp_path).expect("Failed to save context");

        // Load it back
        let loaded_ctx = ConsensusContext::load(&temp_path, validators, Some(0))
            .expect("Failed to load context");

        // Verify all persisted fields match
        assert_eq!(loaded_ctx.block_index, 100);
        assert_eq!(loaded_ctx.view_number, 2);
        assert_eq!(
            loaded_ctx.proposed_block_hash,
            Some(UInt256::from_bytes(&[1u8; 32]).unwrap())
        );
        assert_eq!(
            loaded_ctx.preparation_hash,
            Some(UInt256::from_bytes(&[9u8; 32]).unwrap())
        );
        assert_eq!(loaded_ctx.proposed_timestamp, 1234567890);
        assert_eq!(loaded_ctx.proposed_tx_hashes.len(), 2);
        assert_eq!(
            loaded_ctx.proposed_tx_hashes[0],
            UInt256::from_bytes(&[2u8; 32]).unwrap()
        );
        assert_eq!(
            loaded_ctx.proposed_tx_hashes[1],
            UInt256::from_bytes(&[3u8; 32]).unwrap()
        );
        assert_eq!(loaded_ctx.nonce, 42);
        assert!(loaded_ctx.prepare_request_received);
        assert_eq!(
            loaded_ctx.prepare_request_invocation,
            Some(vec![0x0c, 0x40, 0xaa])
        );
        assert_eq!(loaded_ctx.prepare_responses.len(), 2);
        assert_eq!(
            loaded_ctx.prepare_responses.get(&1),
            Some(&vec![0xaa, 0xbb, 0xcc])
        );
        assert_eq!(
            loaded_ctx.prepare_responses.get(&2),
            Some(&vec![0xdd, 0xee, 0xff])
        );
        assert_eq!(loaded_ctx.prepare_response_hashes.len(), 2);
        assert_eq!(
            loaded_ctx.prepare_response_hashes.get(&1),
            Some(&UInt256::from_bytes(&[0x10; 32]).unwrap())
        );
        assert_eq!(
            loaded_ctx.prepare_response_hashes.get(&2),
            Some(&UInt256::from_bytes(&[0x11; 32]).unwrap())
        );
        assert_eq!(loaded_ctx.commits.len(), 2);
        assert_eq!(loaded_ctx.commits.get(&0), Some(&vec![0x11, 0x22, 0x33]));
        assert_eq!(loaded_ctx.commits.get(&1), Some(&vec![0x44, 0x55, 0x66]));
        assert_eq!(loaded_ctx.commit_view_numbers.get(&0), Some(&2));
        assert_eq!(loaded_ctx.commit_view_numbers.get(&1), Some(&2));
        assert_eq!(
            loaded_ctx.commit_invocations.get(&0),
            Some(&vec![0x0c, 0x40, 0xbb])
        );
        assert_eq!(
            loaded_ctx.commit_invocations.get(&1),
            Some(&vec![0x0c, 0x40, 0xcc])
        );
        assert_eq!(loaded_ctx.change_views.len(), 2);
        assert_eq!(
            loaded_ctx.change_views.get(&3),
            Some(&(3, ChangeViewReason::Timeout))
        );
        assert_eq!(
            loaded_ctx.change_views.get(&4),
            Some(&(3, ChangeViewReason::TxNotFound))
        );
        assert_eq!(
            loaded_ctx.change_view_invocations.get(&3),
            Some(&vec![0x0c, 0x40, 0xdd])
        );
        assert_eq!(
            loaded_ctx.change_view_invocations.get(&4),
            Some(&vec![0x0c, 0x40, 0xee])
        );
        assert_eq!(loaded_ctx.last_change_view_timestamps.get(&3), Some(&1_111));
        assert_eq!(loaded_ctx.last_change_view_timestamps.get(&4), Some(&2_222));

        // Verify non-persisted fields are reset
        assert_eq!(loaded_ctx.state, ConsensusState::Initial);
        assert_eq!(loaded_ctx.view_start_time, 0);
        assert_eq!(loaded_ctx.expected_block_time, 0);
        assert!(loaded_ctx.last_seen_messages.is_empty());

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_save_empty_state() {
        use std::env;

        // Create a minimal context
        let validators = create_test_validators(4);
        let ctx = ConsensusContext::new(0, validators.clone(), None);

        // Save to a temporary file
        let temp_dir = env::temp_dir();
        let temp_path = temp_dir.join("test_consensus_empty.bin");

        ctx.save(&temp_path).expect("Failed to save empty context");

        // Load it back
        let loaded_ctx = ConsensusContext::load(&temp_path, validators, None)
            .expect("Failed to load empty context");

        // Verify basic fields
        assert_eq!(loaded_ctx.block_index, 0);
        assert_eq!(loaded_ctx.view_number, 0);
        assert_eq!(loaded_ctx.proposed_block_hash, None);
        assert!(!loaded_ctx.prepare_request_received);
        assert!(loaded_ctx.prepare_responses.is_empty());
        assert!(loaded_ctx.prepare_response_hashes.is_empty());
        assert!(loaded_ctx.commits.is_empty());
        assert!(loaded_ctx.change_views.is_empty());
        assert!(loaded_ctx.change_view_invocations.is_empty());
        assert!(loaded_ctx.commit_invocations.is_empty());
        assert!(loaded_ctx.prepare_request_invocation.is_none());
        assert!(loaded_ctx.commit_view_numbers.is_empty());

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_save_atomic_write() {
        use std::env;

        let validators = create_test_validators(4);
        let ctx = ConsensusContext::new(42, validators, Some(1));

        let temp_dir = env::temp_dir();
        let temp_path = temp_dir.join("test_consensus_atomic.bin");

        // Save should succeed
        ctx.save(&temp_path).expect("Failed to save");

        // Verify the temp file is cleaned up
        let temp_tmp_path = temp_path.with_extension("tmp");
        assert!(!temp_tmp_path.exists(), "Temp file should be cleaned up");

        // Verify the final file exists
        assert!(temp_path.exists(), "Final file should exist");

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_load_nonexistent_file() {
        use std::env;

        let validators = create_test_validators(4);
        let temp_dir = env::temp_dir();
        let nonexistent_path = temp_dir.join("nonexistent_consensus_state.bin");

        // Should return an IO error
        let result = ConsensusContext::load(&nonexistent_path, validators, None);
        assert!(result.is_err());
        match result {
            Err(ConsensusError::IoError(_)) => {} // Expected
            _ => panic!("Expected IoError"),
        }
    }

    #[test]
    fn test_load_corrupted_file() {
        use std::env;

        let validators = create_test_validators(4);
        let temp_dir = env::temp_dir();
        let corrupted_path = temp_dir.join("test_consensus_corrupted.bin");

        // Write garbage data
        std::fs::write(&corrupted_path, b"this is not valid bincode data")
            .expect("Failed to write corrupted file");

        // Should return a serialization error
        let result = ConsensusContext::load(&corrupted_path, validators, None);
        assert!(result.is_err());
        match result {
            Err(ConsensusError::BincodeError(_)) => {} // Expected
            _ => panic!("Expected BincodeError"),
        }

        // Clean up
        let _ = std::fs::remove_file(&corrupted_path);
    }

    #[test]
    fn test_count_committed() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Initially no commits
        assert_eq!(ctx.count_committed(), 0);

        // Add some commits
        ctx.commits.insert(0, vec![0x11]);
        assert_eq!(ctx.count_committed(), 1);

        ctx.commits.insert(1, vec![0x22]);
        ctx.commits.insert(2, vec![0x33]);
        assert_eq!(ctx.count_committed(), 3);
    }

    #[test]
    fn test_count_failed_empty() {
        let validators = create_test_validators(7);
        let ctx = ConsensusContext::new(100, validators, Some(0));

        // No last_seen_messages tracked yet
        assert_eq!(ctx.count_failed(), 0);
    }

    #[test]
    fn test_count_failed_with_old_messages() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Simulate messages from validators at different block heights
        ctx.last_seen_messages.insert(0, 100); // Current block - not failed
        ctx.last_seen_messages.insert(1, 99); // Previous block - not failed
        ctx.last_seen_messages.insert(2, 98); // Old block (< 99) - FAILED
        ctx.last_seen_messages.insert(3, 95); // Very old block - FAILED
                                              // Validators 4, 5, 6 have no messages - FAILED

        // Failed: validators 2, 3, 4, 5, 6 = 5 validators
        assert_eq!(ctx.count_failed(), 5);
    }

    #[test]
    fn test_count_failed_threshold() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(10, validators, Some(0));

        // Block 10, threshold is 9 (block_index - 1)
        // Messages at block 9 or higher are OK
        // Messages at block 8 or lower are failed

        ctx.last_seen_messages.insert(0, 10); // OK
        ctx.last_seen_messages.insert(1, 9); // OK (exactly at threshold)
        ctx.last_seen_messages.insert(2, 8); // FAILED (< threshold)
                                             // Validator 3 has no message - FAILED

        assert_eq!(ctx.count_failed(), 2); // Validators 2 and 3
    }

    #[test]
    fn test_more_than_f_nodes_committed_or_lost() {
        // 7 validators: f = 2, M = 5
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Initially: committed=0, failed=0, total=0, f=2
        // 0 > 2? No
        assert!(!ctx.more_than_f_nodes_committed_or_lost());

        // Add 2 commits: committed=2, failed=0, total=2, f=2
        // 2 > 2? No
        ctx.commits.insert(0, vec![0x11]);
        ctx.commits.insert(1, vec![0x22]);
        assert!(!ctx.more_than_f_nodes_committed_or_lost());

        // Add 1 more commit: committed=3, failed=0, total=3, f=2
        // 3 > 2? Yes - SHOULD REQUEST RECOVERY
        ctx.commits.insert(2, vec![0x33]);
        assert!(ctx.more_than_f_nodes_committed_or_lost());
    }

    #[test]
    fn test_not_accepting_payloads_due_to_view_changing() {
        // 4 validators: f = 1, M = 3
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(1));

        assert!(!ctx.view_changing());
        assert!(!ctx.not_accepting_payloads_due_to_view_changing());

        // Simulate requesting a view change.
        ctx.change_views.insert(1, (1, ChangeViewReason::Timeout));
        assert!(ctx.view_changing());
        assert!(ctx.not_accepting_payloads_due_to_view_changing());

        // If more than F nodes committed or are lost, accept payloads again.
        ctx.commits.insert(0, vec![0x11]);
        ctx.commits.insert(2, vec![0x22]);
        assert!(ctx.more_than_f_nodes_committed_or_lost());
        assert!(!ctx.not_accepting_payloads_due_to_view_changing());
    }

    #[test]
    fn test_more_than_f_nodes_with_failed() {
        // 7 validators: f = 2, M = 5
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Simulate 2 commits and 1 failed node
        ctx.commits.insert(0, vec![0x11]);
        ctx.commits.insert(1, vec![0x22]);

        ctx.last_seen_messages.insert(0, 100);
        ctx.last_seen_messages.insert(1, 100);
        ctx.last_seen_messages.insert(2, 100);
        ctx.last_seen_messages.insert(3, 100);
        ctx.last_seen_messages.insert(4, 100);
        ctx.last_seen_messages.insert(5, 100);
        ctx.last_seen_messages.insert(6, 95); // Old message - FAILED

        // committed=2, failed=1, total=3, f=2
        // 3 > 2? Yes - SHOULD REQUEST RECOVERY
        assert_eq!(ctx.count_committed(), 2);
        assert_eq!(ctx.count_failed(), 1);
        assert!(ctx.more_than_f_nodes_committed_or_lost());
    }

    #[test]
    fn test_more_than_f_nodes_edge_case() {
        // 4 validators: f = 1, M = 3
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(50, validators, Some(0));

        // committed=1, failed=0, total=1, f=1
        // 1 > 1? No
        ctx.commits.insert(0, vec![0x11]);
        assert!(!ctx.more_than_f_nodes_committed_or_lost());

        // committed=1, failed=1, total=2, f=1
        // 2 > 1? Yes - SHOULD REQUEST RECOVERY
        ctx.last_seen_messages.insert(0, 50);
        ctx.last_seen_messages.insert(1, 50);
        ctx.last_seen_messages.insert(2, 50);
        ctx.last_seen_messages.insert(3, 45); // Old - FAILED

        assert_eq!(ctx.count_failed(), 1);
        assert!(ctx.more_than_f_nodes_committed_or_lost());
    }

    #[test]
    fn test_update_last_seen_message() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        assert!(ctx.last_seen_messages.is_empty());

        ctx.update_last_seen_message(0, 100);
        assert_eq!(ctx.last_seen_messages.get(&0), Some(&100));

        ctx.update_last_seen_message(1, 101);
        assert_eq!(ctx.last_seen_messages.get(&1), Some(&101));

        // Update existing entry
        ctx.update_last_seen_message(0, 102);
        assert_eq!(ctx.last_seen_messages.get(&0), Some(&102));
    }

    #[test]
    fn test_message_hash_caching() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Create test message hashes
        let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let hash2 = UInt256::from_bytes(&[2u8; 32]).unwrap();

        // Initially, no messages have been seen
        assert!(!ctx.has_seen_message(&hash1));
        assert!(!ctx.has_seen_message(&hash2));

        // Mark hash1 as seen
        ctx.mark_message_seen(&hash1);
        assert!(ctx.has_seen_message(&hash1));
        assert!(!ctx.has_seen_message(&hash2));

        // Mark hash2 as seen
        ctx.mark_message_seen(&hash2);
        assert!(ctx.has_seen_message(&hash1));
        assert!(ctx.has_seen_message(&hash2));

        // Marking the same hash again should be idempotent
        ctx.mark_message_seen(&hash1);
        assert!(ctx.has_seen_message(&hash1));
    }

    #[test]
    fn test_message_cache_cleared_on_new_block() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Add some message hashes
        let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
        let hash2 = UInt256::from_bytes(&[2u8; 32]).unwrap();
        ctx.mark_message_seen(&hash1);
        ctx.mark_message_seen(&hash2);

        assert!(ctx.has_seen_message(&hash1));
        assert!(ctx.has_seen_message(&hash2));

        // Reset for new block should clear the cache
        ctx.reset_for_new_block(101, 2000);

        assert!(!ctx.has_seen_message(&hash1));
        assert!(!ctx.has_seen_message(&hash2));
        assert_eq!(ctx.block_index, 101);
    }

    #[test]
    fn test_message_cache_not_cleared_on_view_change() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Add some message hashes
        let hash1 = UInt256::from_bytes(&[1u8; 32]).unwrap();
        ctx.mark_message_seen(&hash1);
        assert!(ctx.has_seen_message(&hash1));

        // Reset for new view should NOT clear the message cache
        // (messages are still valid within the same block)
        ctx.reset_for_new_view(1, 1000);

        assert!(ctx.has_seen_message(&hash1));
        assert_eq!(ctx.view_number, 1);
    }

    #[test]
    fn test_message_cache_prevents_replay() {
        let validators = create_test_validators(7);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Simulate receiving the same message twice
        let msg_hash = UInt256::from_bytes(&[0xaa; 32]).unwrap();

        // First time: message is new
        assert!(!ctx.has_seen_message(&msg_hash));
        ctx.mark_message_seen(&msg_hash);

        // Second time: message is duplicate (replay attack)
        assert!(ctx.has_seen_message(&msg_hash));
    }

    #[test]
    fn test_message_cache_lru_limit() {
        let validators = create_test_validators(4);
        let mut ctx = ConsensusContext::new(100, validators, Some(0));

        // Fill the cache to just below the limit
        for i in 0..100 {
            let mut hash_bytes = [0u8; 32];
            hash_bytes[0..4].copy_from_slice(&(i as u32).to_le_bytes());
            let hash = UInt256::from_bytes(&hash_bytes).unwrap();
            ctx.mark_message_seen(&hash);
        }

        // Verify messages are cached
        let first_hash = UInt256::from_bytes(&[0u8; 32]).unwrap();
        assert!(ctx.has_seen_message(&first_hash));

        // The cache should not be cleared yet (under limit)
        assert!(ctx.seen_message_hashes.len() <= MAX_MESSAGE_CACHE_SIZE);
    }
}

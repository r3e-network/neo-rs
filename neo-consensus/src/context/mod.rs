//! Consensus context - tracks the current consensus state.

use crate::{ChangeViewReason, ConsensusError, ConsensusResult};
use lru::LruCache;
#[cfg(test)]
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;

/// Default block time in milliseconds (15 seconds for Neo N3).
/// Post-Echidna, `MillisecondsPerBlock` is a committee-configurable policy
/// setting.  Use this only as a fallback when no policy value is available.
pub const DEFAULT_BLOCK_TIME_MS: u64 = 15_000;

/// Backwards-compatible alias (deprecated - prefer `DEFAULT_BLOCK_TIME_MS`).
pub const BLOCK_TIME_MS: u64 = DEFAULT_BLOCK_TIME_MS;

/// Maximum validators in dBFT
pub const MAX_VALIDATORS: usize = 21;

/// Maximum size of message hash cache (LRU limit for memory protection)
/// Matches C# `DBFTPlugin`'s message caching behavior
pub const MAX_MESSAGE_CACHE_SIZE: usize = 10_000;

mod persistence;
mod state;
mod timer;
mod validator_info;

use persistence::{PersistedConsensusState, decode_state, encode_state};
pub use state::ConsensusState;
pub use validator_info::ValidatorInfo;

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
    /// Extra milliseconds added to the current view's change-view deadline by
    /// `extend_timer_by_factor` (C# `ExtendTimerByFactor`). Reset to 0 on every
    /// new view/block. Never decreases within a view.
    pub timer_extension: u64,
    /// Expected block time
    pub expected_block_time: u64,

    // Proposal data
    /// Block version (must be 0 for Neo N3)
    pub version: u32,
    /// Previous block hash for the proposed block.
    pub prev_hash: UInt256,
    /// Previous block timestamp in milliseconds.
    pub previous_block_timestamp: u64,
    /// Header `NextConsensus` address for the proposed block.
    pub next_consensus: UInt160,
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
    /// Proposed transaction hashes that are locally available for this view.
    pub available_tx_hashes: HashSet<UInt256>,
    /// Nonce for the block
    pub nonce: u64,

    // Signature tracking
    /// `PrepareRequest` received from primary
    pub prepare_request_received: bool,
    /// Primary has asked the node/mempool for transactions for this view.
    pub transaction_request_sent: bool,
    /// Timestamp when the primary transaction request timer fired.
    pub transaction_request_sent_at: Option<u64>,
    /// Timestamp when commit recovery was last resent after local commit.
    pub commit_recovery_sent_at: Option<u64>,
    /// Earliest timestamp when this node may resend a ChangeView for the current view.
    pub change_view_retry_at: Option<u64>,
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
    /// Transactions reported invalid this block round (C# `InvalidTransactions`):
    /// tx hash -> set of validator indices that flagged it via a
    /// `TxRejectedByPolicy`/`TxInvalid` `ChangeView`. The primary skips a tx
    /// whose count exceeds `F`. Keyed by index (1:1 with the C# `ECPoint` set
    /// within a round, so the count — and the skip decision — is identical).
    /// Accumulates across views; cleared on a new block.
    pub invalid_transactions: HashMap<UInt256, HashSet<u8>>,
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
    seen_message_hashes: LruCache<UInt256, ()>,
}

impl ConsensusContext {
    /// Creates a new consensus context.
    ///
    /// `block_time_ms` sets the expected block interval. Pass `None` (or `0`)
    /// to fall back to [`DEFAULT_BLOCK_TIME_MS`].
    #[must_use]
    pub fn new(
        block_index: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        block_time_ms: Option<u64>,
    ) -> Self {
        let effective_block_time = match block_time_ms {
            Some(t) if t > 0 => t,
            _ => DEFAULT_BLOCK_TIME_MS,
        };
        Self {
            block_index,
            view_number: 0,
            validators,
            my_index,
            state: ConsensusState::Initial,
            view_start_time: 0,
            timer_extension: 0,
            expected_block_time: effective_block_time,
            version: 0,
            prev_hash: UInt256::zero(),
            previous_block_timestamp: 0,
            next_consensus: UInt160::zero(),
            proposed_block_hash: None,
            preparation_hash: None,
            proposed_timestamp: 0,
            proposed_tx_hashes: Vec::new(),
            available_tx_hashes: HashSet::new(),
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

    fn new_seen_message_cache() -> LruCache<UInt256, ()> {
        LruCache::new(
            NonZeroUsize::new(MAX_MESSAGE_CACHE_SIZE)
                .expect("message cache capacity must be non-zero"),
        )
    }

    /// Returns the number of validators
    #[must_use]
    pub fn validator_count(&self) -> usize {
        self.validators.len()
    }

    /// Returns the number of faulty nodes tolerated: f = (n-1)/3
    #[must_use]
    pub fn f(&self) -> usize {
        self.validator_count().saturating_sub(1) / 3
    }

    /// Returns the number of nodes required for consensus: M = n - f
    #[must_use]
    pub fn m(&self) -> usize {
        self.validator_count().saturating_sub(self.f())
    }

    /// Records that `validator_index` reported `hashes` as invalid (from a
    /// `TxRejectedByPolicy`/`TxInvalid` `ChangeView`). C# `InvalidTransactions`
    /// population (ConsensusService.OnMessage).
    pub fn record_invalid_transactions(&mut self, validator_index: u8, hashes: &[UInt256]) {
        for hash in hashes {
            self.invalid_transactions
                .entry(*hash)
                .or_default()
                .insert(validator_index);
        }
    }

    /// Transaction hashes that MORE THAN `F` validators have reported invalid —
    /// the primary must skip these when building the block (C#
    /// `EnsureMaxBlockLimitation`: `if (InvalidTransactions[hash].Count > F) continue`).
    #[must_use]
    pub fn invalid_tx_hashes_over_f(&self) -> Vec<UInt256> {
        let f = self.f();
        self.invalid_transactions
            .iter()
            .filter(|(_, reporters)| reporters.len() > f)
            .map(|(hash, _)| *hash)
            .collect()
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
        self.timer_extension = 0;
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
        self.available_tx_hashes.clear();
        self.nonce = 0;

        // Clear signatures
        self.prepare_request_received = false;
        self.transaction_request_sent = false;
        self.transaction_request_sent_at = None;
        self.commit_recovery_sent_at = None;
        self.change_view_retry_at = None;
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
        self.timer_extension = 0;
        self.state = if self.is_primary() {
            ConsensusState::Primary
        } else {
            ConsensusState::Backup
        };

        // Clear all data
        self.version = 0;
        self.prev_hash = UInt256::zero();
        self.previous_block_timestamp = 0;
        self.next_consensus = UInt160::zero();
        self.proposed_block_hash = None;
        self.preparation_hash = None;
        self.proposed_timestamp = 0;
        self.proposed_tx_hashes.clear();
        self.available_tx_hashes.clear();
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

    /// Records which proposed transactions are locally available.
    pub fn mark_available_transactions<I>(&mut self, tx_hashes: I)
    where
        I: IntoIterator<Item = UInt256>,
    {
        self.available_tx_hashes.clear();
        for hash in tx_hashes {
            if self.proposed_tx_hashes.contains(&hash) {
                self.available_tx_hashes.insert(hash);
            }
        }
    }

    /// Returns true when a proposal references transactions this node has not received.
    #[must_use]
    pub fn has_missing_proposed_transactions(&self) -> bool {
        !self.proposed_tx_hashes.is_empty()
            && self.proposed_tx_hashes.len() > self.available_tx_hashes.len()
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
    /// Security: uses a bounded LRU cache (`MAX_MESSAGE_CACHE_SIZE`) to prevent memory
    /// exhaustion attacks while avoiding a clear-all window for recently seen messages.
    ///
    /// # Arguments
    /// * `hash` - The message hash to mark as seen
    pub fn mark_message_seen(&mut self, hash: &UInt256) {
        if self.seen_message_hashes.contains(hash) {
            return;
        }
        self.seen_message_hashes.put(*hash, ());
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

        let encoded = encode_state(&state)?;

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
        encode_state(&state)
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
        let state = decode_state(encoded)?;

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
            timer_extension: 0,
            expected_block_time: 0, // Caller should update
            version: 0,
            prev_hash: UInt256::zero(),
            previous_block_timestamp: 0,
            next_consensus: UInt160::zero(),
            proposed_block_hash: state.proposed_block_hash,
            preparation_hash: state.preparation_hash,
            proposed_timestamp: state.proposed_timestamp,
            proposed_tx_hashes: state.proposed_tx_hashes,
            available_tx_hashes: HashSet::new(),
            nonce: state.nonce,
            prepare_request_received: state.prepare_request_received,
            transaction_request_sent: false,
            transaction_request_sent_at: None,
            commit_recovery_sent_at: None,
            change_view_retry_at: None,
            prepare_responses: state.prepare_responses,
            prepare_response_hashes: state.prepare_response_hashes,
            commits: state.commits,
            commit_view_numbers: state.commit_view_numbers,
            change_views: state.change_views,
            // Ephemeral per-round state, not persisted — restarts empty on reload.
            invalid_transactions: HashMap::new(),
            prepare_request_invocation: state.prepare_request_invocation,
            change_view_invocations: state.change_view_invocations,
            commit_invocations: state.commit_invocations,
            last_change_view_timestamps: state.change_view_timestamps,
            last_seen_messages: HashMap::new(), // Not persisted
            seen_message_hashes: Self::new_seen_message_cache(), // Not persisted
        })
    }
}

#[cfg(test)]
#[path = "../tests/context.rs"]
mod tests;

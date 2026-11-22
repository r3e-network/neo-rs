//! Consensus context - tracks the current consensus state.

use crate::{ChangeViewReason, ConsensusError, ConsensusResult};
use lru::LruCache;
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
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

/// Number of Byzantine validators dBFT tolerates for `validator_count` nodes:
/// `f = (n - 1) / 3` (C# `ConsensusContext.F`).
#[must_use]
pub fn fault_tolerance(validator_count: usize) -> usize {
    validator_count.saturating_sub(1) / 3
}

/// Signatures a dBFT block needs from `validator_count` validators:
/// `M = n - f` with `f = (n - 1) / 3` (C# `ConsensusContext.M`).
///
/// This is the quorum used for the multi-sig witness, so block-size accounting
/// and witness construction must both take it from here.
#[must_use]
pub fn required_signatures(validator_count: usize) -> usize {
    validator_count - fault_tolerance(validator_count)
}

/// Default value of `ProtocolSettings.MaxTransactionsPerBlock` on Neo N3.
/// Used to reject `PrepareRequest` proposals that carry more transactions than
/// a block may hold (C# `OnPrepareRequestReceived`).
pub const DEFAULT_MAX_TRANSACTIONS_PER_BLOCK: usize = 512;

/// C# `OnPrepareRequestReceived` rejects proposals whose timestamp is more than
/// `8 * MillisecondsPerBlock` in the future.
pub const MAX_PREPARE_REQUEST_FUTURE_MS_FACTOR: u64 = 8;

/// Maximum size of message hash cache (LRU limit for memory protection)
/// Matches C# `DBFTPlugin`'s message caching behavior
pub const MAX_MESSAGE_CACHE_SIZE: usize = 10_000;

/// Maximum number of verification-rejected message hashes retained at once.
/// Deliberately far smaller than [`MAX_MESSAGE_CACHE_SIZE`]: rejected hashes
/// are attacker-supplied, so the cache they populate must stay cheap.
pub const MAX_REJECTED_MESSAGE_CACHE_SIZE: usize = 1_024;

/// Maximum number of distinct rejected transaction hashes a single validator
/// may contribute to `rejected_tx_validators` within one block.
///
/// SECURITY: rejected hashes travel in `ChangeView` messages, so they are
/// attacker-controlled. C# keys `InvalidTransactions` by validator public key
/// without a bound; here the per-validator budget keeps the round state
/// bounded (at most `MAX_VALIDATORS * budget` retained hashes) while leaving
/// room for every transaction a block can legitimately contain.
pub const MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK: usize = 1_000;

/// How long (ms) a verification-rejected message hash suppresses replays of
/// that payload. The hash covers only the unsigned payload bytes, so a longer
/// TTL would also suppress a legitimate retransmission carrying a different
/// (valid) signature over the same unsigned data; this window bounds that
/// availability cost while still capping replays to one secp256r1
/// verification per interval per hash.
pub const REJECTED_MESSAGE_TTL_MS: u64 = 10_000;

/// Consensus state enumeration
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

/// Durable authorization for publishing a locally generated Commit signature.
///
/// The recovery bytes are deliberately carried out-of-band and are never part
/// of the dBFT wire format. An egress layer must validate and durably persist
/// these exact bytes before exposing the signature.
#[derive(Debug, Clone)]
pub struct CommitPersistencePermit {
    /// Block index bound to the Commit.
    pub block_index: u32,
    /// View number bound to the Commit.
    pub view_number: u8,
    /// Local validator index that produced the Commit.
    pub validator_index: u8,
    /// Verified proposal hash signed by the Commit.
    pub proposal_hash: UInt256,
    /// Exact block-hash signature that may be published.
    pub signature: Vec<u8>,
    /// Exact recovery record that must be durably persisted.
    pub recovery_state: Vec<u8>,
}

impl CommitPersistencePermit {
    /// Validates that the recovery record contains this exact Commit identity.
    pub fn validate(&self) -> ConsensusResult<()> {
        if self.signature.len() != 64 {
            return Err(ConsensusError::Persistence(
                "commit persistence permit has an invalid signature length".to_string(),
            ));
        }
        let state = ConsensusContext::decode_state(&self.recovery_state)?;
        let matches = state.block_index == self.block_index
            && state.view_number == self.view_number
            && state.proposed_block_hash == Some(self.proposal_hash)
            && state.commits.get(&self.validator_index) == Some(&self.signature)
            && state.commit_view_numbers.get(&self.validator_index) == Some(&self.view_number);
        if !matches {
            return Err(ConsensusError::Persistence(
                "commit persistence permit does not match its recovery record".to_string(),
            ));
        }
        Ok(())
    }

    /// Returns whether this permit authorizes the exact Commit signature.
    #[must_use]
    pub fn covers_commit(
        &self,
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        signature: &[u8],
    ) -> bool {
        self.block_index == block_index
            && self.view_number == view_number
            && self.validator_index == validator_index
            && self.signature == signature
            && self.validate().is_ok()
    }
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
    /// Timestamp when the current timer started.
    pub view_start_time: u64,
    /// Expected block time
    pub expected_block_time: u64,
    /// Explicit timer duration, when the timer was changed independently of the view.
    /// This is transient and is intentionally not persisted with consensus state.
    timer_timeout: Option<u64>,
    /// Whether the primary's initial timer has fired and transactions were requested.
    /// This is transient and prevents a mempool callback from sending early.
    proposal_requested: bool,

    // Proposal data
    /// Block version (must be 0 for Neo N3)
    pub version: u32,
    /// Previous block hash for the proposed block.
    pub prev_hash: UInt256,
    /// Timestamp of the previous block header, used to reject `PrepareRequest`s
    /// whose timestamp is not strictly greater than their parent's.
    /// `0` means "unknown" and disables that check.
    pub prev_timestamp: u64,
    /// Upper bound on the number of transactions a `PrepareRequest` may propose.
    pub max_transactions_per_block: usize,
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
    /// Whether this node holds and has verified every transaction in
    /// `proposed_tx_hashes`. C# refuses to sign a Commit or assemble a block
    /// before `TransactionHashes.All(p => Transactions.ContainsKey(p))`.
    ///
    /// This is transient: after a restart the transactions must be re-fetched
    /// and re-validated before this node signs again.
    pub transactions_available: bool,
    /// Validators that rejected each transaction in a `ChangeView` (C#
    /// `InvalidTransactions`: hash -> set of validator public keys).
    pub rejected_tx_validators: HashMap<UInt256, std::collections::HashSet<u8>>,
    /// Number of distinct hashes each validator already contributed to
    /// `rejected_tx_validators` in this block. Enforces
    /// [`MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK`].
    rejected_hash_counts: HashMap<u8, usize>,
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
    seen_message_hashes: LruCache<UInt256, ()>,
    /// Verification-rejected message hashes (`hash` -> rejection timestamp in
    /// ms). Replay guard for payloads whose signature verification failed, so
    /// retransmitting them cannot force repeated secp256r1 work.
    rejected_message_hashes: LruCache<UInt256, u64>,
    /// Payload hashes for which this node already sent a recovery response.
    /// C# keeps these hashes for the whole block, including stale ChangeView payloads.
    recovery_response_hashes: LruCache<UInt256, ()>,
    /// Last sealed local Commit permit, retained across view changes so a
    /// RecoveryMessage cannot expose an earlier-view signature without proof.
    local_commit_persistence: Option<CommitPersistencePermit>,
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
        assert!(
            validators.len() <= MAX_VALIDATORS,
            "validator count {} exceeds MAX_VALIDATORS ({})",
            validators.len(),
            MAX_VALIDATORS
        );
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
            expected_block_time: effective_block_time,
            timer_timeout: None,
            proposal_requested: false,
            version: 0,
            prev_hash: UInt256::zero(),
            prev_timestamp: 0,
            max_transactions_per_block: DEFAULT_MAX_TRANSACTIONS_PER_BLOCK,
            proposed_block_hash: None,
            preparation_hash: None,
            proposed_timestamp: 0,
            proposed_tx_hashes: Vec::new(),
            transactions_available: false,
            rejected_tx_validators: HashMap::new(),
            rejected_hash_counts: HashMap::new(),
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
            seen_message_hashes: Self::new_seen_message_cache(),
            rejected_message_hashes: Self::new_rejected_message_cache(),
            recovery_response_hashes: Self::new_seen_message_cache(),
            local_commit_persistence: None,
        }
    }

    fn new_rejected_message_cache() -> LruCache<UInt256, u64> {
        LruCache::new(
            NonZeroUsize::new(MAX_REJECTED_MESSAGE_CACHE_SIZE)
                .expect("rejected message cache capacity must be non-zero"),
        )
    }

    fn new_seen_message_cache() -> LruCache<UInt256, ()> {
        LruCache::new(
            NonZeroUsize::new(MAX_MESSAGE_CACHE_SIZE)
                .expect("message cache capacity must be non-zero"),
        )
    }
}

mod mutations;
mod queries;

#[cfg(test)]
mod tests;

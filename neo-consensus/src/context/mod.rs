//! # neo-consensus::context
//!
//! Runtime context records carried through the local workflow.
//!
//! ## Boundary
//!
//! This module belongs to `neo-consensus`. This protocol/service crate owns
//! dBFT state and messages and must not own ledger persistence, RPC transport,
//! or application startup.
//!
//! ## Contents
//!
//! - `liveness`: validator liveness, failure, and view-change guards.
//! - `persistence`: Persistence traits, snapshots, transactions, and cache
//!   overlays.
//! - `quorum`: validator counts, speaker role, dBFT thresholds, and quorum
//!   checks.
//! - `replay`: bounded message-hash replay protection.
//! - `round`: view/block lifecycle resets.
//! - `signatures`: prepare, commit, and change-view payload mutation helpers.
//! - `state`: domain state records for the surrounding workflow.
//! - `timer`: consensus timer policy and scheduling helpers.
//! - `transactions`: proposal transaction availability and block-policy math.
//! - `validator_info`: validator metadata records.
//! - `tests`: Module-local tests and regression coverage.

use crate::ChangeViewReason;
use lru::LruCache;
#[cfg(test)]
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use std::collections::{HashMap, HashSet};

/// Default block time in milliseconds (15 seconds for Neo N3).
/// Post-Echidna, `MillisecondsPerBlock` is a committee-configurable policy
/// setting.  Use this only as a fallback when no policy value is available.
pub const DEFAULT_BLOCK_TIME_MS: u64 = 15_000;

/// Backwards-compatible alias (deprecated - prefer `DEFAULT_BLOCK_TIME_MS`).
pub const BLOCK_TIME_MS: u64 = DEFAULT_BLOCK_TIME_MS;

/// Maximum validators in dBFT
pub const MAX_VALIDATORS: usize = 21;

/// C# DBFTPlugin `DbftSettings.MaxBlockSize` — the block-size policy a backup
/// enforces in `CheckPrepareResponse` before sending its `PrepareResponse`.
/// The DBFTPlugin ships `MaxBlockSize = 2097152` (2 MiB) in `DBFTPlugin.json`,
/// which matches `neo_primitives::constants::MAX_BLOCK_SIZE` and the value the
/// primary already enforces during `EnsureMaxBlockLimitation`. Used as the
/// default until the node overrides it via
/// [`ConsensusContext::set_max_block_policy`].
pub const DEFAULT_MAX_BLOCK_SIZE: u32 = 2_097_152;

/// C# DBFTPlugin `DbftSettings.MaxBlockSystemFee` default (150000000000, i.e.
/// 1500 GAS). The block-system-fee policy a backup enforces in
/// `CheckPrepareResponse`, identical to the limit the primary applies in
/// `EnsureMaxBlockLimitation`.
pub const DEFAULT_MAX_BLOCK_SYSTEM_FEE: i64 = 150_000_000_000;

/// Maximum size of message hash cache (LRU limit for memory protection)
/// Matches C# `DBFTPlugin`'s message caching behavior
pub const MAX_MESSAGE_CACHE_SIZE: usize = 10_000;

mod liveness;
mod persistence;
mod quorum;
mod replay;
mod round;
mod signatures;
mod state;
mod timer;
mod transactions;
mod validator_info;

pub use state::ConsensusState;
pub use transactions::TxMetrics;
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
    /// Per-transaction (wire size, system fee) for the locally available
    /// proposal transactions (C# `ConsensusContext.Transactions.Values`, from
    /// which `GetExpectedBlockSize` sums `tx.Size` and `GetExpectedBlockSystemFee`
    /// sums `tx.SystemFee`). The service crate is otherwise hash-only; the node
    /// feeds these metrics as it caches each transaction body so the backup can
    /// evaluate the block-policy limits in `CheckPrepareResponse` without pulling
    /// full transaction bodies into the consensus crate.
    pub available_tx_metrics: HashMap<UInt256, TxMetrics>,
    /// Block-size policy limit (C# `DbftSettings.MaxBlockSize`). Enforced by a
    /// backup in `send_prepare_response` (C# `CheckPrepareResponse`).
    pub max_block_size: u32,
    /// Block-system-fee policy limit (C# `DbftSettings.MaxBlockSystemFee`).
    pub max_block_system_fee: i64,
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

#[cfg(test)]
#[path = "../tests/context/mod.rs"]
mod tests;

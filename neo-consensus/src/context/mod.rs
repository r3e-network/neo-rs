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
//! - `construction`: fresh context defaults for a new dBFT round.
//! - `liveness`: validator liveness, failure, and view-change guards.
//! - `persistence`: Persistence traits, snapshots, transactions, and cache
//!   overlays.
//! - `policy`: dBFT context defaults and bounded-cache limits.
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

mod construction;
mod liveness;
mod persistence;
mod policy;
mod quorum;
mod replay;
mod round;
mod signatures;
mod state;
mod timer;
mod transactions;
mod validator_info;

pub use policy::{
    BLOCK_TIME_MS, DEFAULT_BLOCK_TIME_MS, DEFAULT_MAX_BLOCK_SIZE, DEFAULT_MAX_BLOCK_SYSTEM_FEE,
    MAX_MESSAGE_CACHE_SIZE, MAX_VALIDATORS,
};
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

#[cfg(test)]
#[path = "../tests/context/mod.rs"]
mod tests;

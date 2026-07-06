use crate::{ChangeViewReason, ConsensusResult};
use neo_primitives::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use super::{
    ConsensusContext, ConsensusState, DEFAULT_MAX_BLOCK_SIZE, DEFAULT_MAX_BLOCK_SYSTEM_FEE,
    ValidatorInfo,
};

/// Persisted consensus state for crash recovery.
///
/// Contains only the essential state needed to resume consensus after a restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedConsensusState {
    /// Current block index being proposed.
    pub(super) block_index: u32,
    /// Current view number (increments on view change).
    pub(super) view_number: u8,
    /// Proposed block hash from `PrepareRequest`.
    pub(super) proposed_block_hash: Option<UInt256>,
    /// Hash of the primary `PrepareRequest` payload (`ExtensiblePayload.Hash`).
    #[serde(default)]
    pub(super) preparation_hash: Option<UInt256>,
    /// Proposed block timestamp.
    pub(super) proposed_timestamp: u64,
    /// Proposed transaction hashes.
    pub(super) proposed_tx_hashes: Vec<UInt256>,
    /// Nonce for the block.
    pub(super) nonce: u64,
    /// Whether `PrepareRequest` was received from primary.
    pub(super) prepare_request_received: bool,
    /// `PrepareResponse` signatures (`validator_index` -> signature).
    pub(super) prepare_responses: HashMap<u8, Vec<u8>>,
    /// `PrepareResponse` hashes (`validator_index` -> `preparation_hash`).
    #[serde(default)]
    pub(super) prepare_response_hashes: HashMap<u8, UInt256>,
    /// Commit signatures (`validator_index` -> signature).
    pub(super) commits: HashMap<u8, Vec<u8>>,
    /// Commit view numbers (`validator_index` -> `view_number`).
    #[serde(default)]
    pub(super) commit_view_numbers: HashMap<u8, u8>,
    /// `ChangeView` requests (`validator_index` -> (`new_view`, reason)).
    pub(super) change_views: HashMap<u8, (u8, ChangeViewReason)>,
    /// Primary `PrepareRequest` invocation script (payload witness).
    #[serde(default)]
    pub(super) prepare_request_invocation: Option<Vec<u8>>,
    /// `ChangeView` invocation script per validator (payload witness).
    #[serde(default)]
    pub(super) change_view_invocations: HashMap<u8, Vec<u8>>,
    /// `ChangeView` timestamp per validator.
    #[serde(default)]
    pub(super) change_view_timestamps: HashMap<u8, u64>,
    /// Commit invocation script per validator (payload witness).
    #[serde(default)]
    pub(super) commit_invocations: HashMap<u8, Vec<u8>>,
}

pub(super) fn encode_state(state: &PersistedConsensusState) -> ConsensusResult<Vec<u8>> {
    Ok(bincode::serialize(state)?)
}

pub(super) fn decode_state(encoded: &[u8]) -> ConsensusResult<PersistedConsensusState> {
    Ok(bincode::deserialize(encoded)?)
}

impl ConsensusContext {
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
            // Per-tx metrics are ephemeral (fed by the node as bodies are
            // cached); a reloaded backup re-fills them as it re-fetches the
            // proposal transactions. Policy limits restart at the C# defaults
            // and are re-set by the node when the round is configured.
            available_tx_metrics: HashMap::new(),
            max_block_size: DEFAULT_MAX_BLOCK_SIZE,
            max_block_system_fee: DEFAULT_MAX_BLOCK_SYSTEM_FEE,
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

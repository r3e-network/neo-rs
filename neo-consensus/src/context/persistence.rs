use crate::{ChangeViewReason, ConsensusResult};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

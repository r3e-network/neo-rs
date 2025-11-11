use alloc::vec::Vec;

use neo_base::hash::Hash256;
use serde::{Deserialize, Serialize};

use crate::message::types::{ChangeViewReason, MessageKind, ViewNumber};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusMessage {
    PrepareRequest {
        proposal_hash: Hash256,
        height: u64,
        tx_hashes: Vec<Hash256>,
    },
    PrepareResponse {
        proposal_hash: Hash256,
    },
    Commit {
        proposal_hash: Hash256,
    },
    ChangeView {
        new_view: ViewNumber,
        reason: ChangeViewReason,
        timestamp_ms: u64,
    },
}

impl ConsensusMessage {
    pub fn kind(&self) -> MessageKind {
        match self {
            Self::PrepareRequest { .. } => MessageKind::PrepareRequest,
            Self::PrepareResponse { .. } => MessageKind::PrepareResponse,
            Self::Commit { .. } => MessageKind::Commit,
            Self::ChangeView { .. } => MessageKind::ChangeView,
        }
    }

    pub fn proposal_hash(&self) -> Option<Hash256> {
        match self {
            Self::PrepareRequest { proposal_hash, .. }
            | Self::PrepareResponse { proposal_hash }
            | Self::Commit { proposal_hash } => Some(*proposal_hash),
            Self::ChangeView { .. } => None,
        }
    }
}

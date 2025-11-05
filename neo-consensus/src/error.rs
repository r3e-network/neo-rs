use neo_base::hash::Hash256;
use thiserror::Error;

use crate::{
    message::{MessageKind, ViewNumber},
    validator::ValidatorId,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConsensusError {
    #[error("message height {received} does not match expected {expected}")]
    InvalidHeight { expected: u64, received: u64 },

    #[error("message signed by unknown validator {0:?}")]
    UnknownValidator(ValidatorId),

    #[error("validator set is empty")]
    NoValidators,

    #[error("signature verification failed for validator {0:?}")]
    InvalidSignature(ValidatorId),

    #[error("proposal hash mismatch expected {expected:?} got {actual:?}")]
    ProposalMismatch { expected: Hash256, actual: Hash256 },

    #[error("message view {received:?} does not match expected {expected:?}")]
    InvalidView {
        expected: ViewNumber,
        received: ViewNumber,
    },

    #[error("{kind:?} from view {message_view:?} is stale; current view is {current_view:?}")]
    StaleMessage {
        kind: MessageKind,
        current_view: ViewNumber,
        message_view: ViewNumber,
    },

    #[error("change view request {requested:?} must be greater than current view {current:?}")]
    StaleView {
        current: ViewNumber,
        requested: ViewNumber,
    },

    #[error("change view target mismatch expected {expected:?} got {received:?}")]
    InconsistentView {
        expected: ViewNumber,
        received: ViewNumber,
    },

    #[error("proposal not yet registered")]
    MissingProposal,

    #[error("message must originate from primary {expected:?}, received {actual:?}")]
    InvalidPrimary {
        expected: ValidatorId,
        actual: ValidatorId,
    },

    #[error("message {kind:?} already recorded for validator {validator:?}")]
    DuplicateMessage {
        kind: MessageKind,
        validator: ValidatorId,
    },

    #[error("validator {validator:?} must issue PrepareResponse before Commit")]
    MissingPrepareResponse { validator: ValidatorId },

    #[error("quorum not reached for {0:?}")]
    QuorumNotReached(MessageKind),

    #[error("cannot transition from height {current} to {requested}")]
    InvalidHeightTransition { current: u64, requested: u64 },
}

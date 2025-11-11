use alloc::vec::Vec;

use neo_base::hash::Hash256;

use crate::{
    message::{MessageKind, ViewNumber},
    validator::ValidatorId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuorumDecision {
    Pending,
    Proposal {
        kind: MessageKind,
        proposal: Hash256,
        missing: Vec<ValidatorId>,
    },
    ViewChange {
        new_view: ViewNumber,
        missing: Vec<ValidatorId>,
    },
}

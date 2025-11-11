use alloc::{collections::BTreeMap, vec::Vec};

use neo_base::hash::Hash256;

use crate::{
    message::{ChangeViewReason, MessageKind, SignedMessage, ViewNumber},
    validator::ValidatorId,
};

use super::super::core::ConsensusState;

/// Compact representation suitable for snapshotting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotState {
    pub height: u64,
    pub view: ViewNumber,
    pub proposal: Option<Hash256>,
    pub participation: BTreeMap<MessageKind, Vec<SignedMessage>>,
    pub expected: BTreeMap<MessageKind, Vec<ValidatorId>>,
    pub change_view_reasons: BTreeMap<ValidatorId, ChangeViewReason>,
    pub change_view_reason_counts: BTreeMap<ChangeViewReason, usize>,
    pub change_view_total: u64,
}

impl From<&ConsensusState> for SnapshotState {
    fn from(state: &ConsensusState) -> Self {
        let mut participation = BTreeMap::new();
        for (kind, messages) in state.records.iter() {
            participation.insert(*kind, messages.clone());
        }

        let mut expected = BTreeMap::new();
        for (kind, validators) in state.expected.iter() {
            expected.insert(*kind, validators.clone());
        }

        let reasons = state.change_view_reasons();
        let reason_counts = state.change_view_reason_counts();

        Self {
            height: state.height,
            view: state.view,
            proposal: state.proposal,
            participation,
            expected,
            change_view_reasons: reasons,
            change_view_reason_counts: reason_counts,
            change_view_total: state.change_view_total,
        }
    }
}

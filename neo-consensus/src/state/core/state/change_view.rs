use alloc::collections::BTreeMap;

use super::ConsensusState;
use crate::{
    message::{ChangeViewReason, ConsensusMessage, MessageKind, ViewNumber},
    validator::ValidatorId,
};

impl ConsensusState {
    pub fn change_view_reasons(&self) -> BTreeMap<ValidatorId, ChangeViewReason> {
        let mut reasons = BTreeMap::new();
        for (validator, reason) in &self.change_view_reasons {
            reasons.insert(*validator, *reason);
        }
        reasons
    }

    pub fn change_view_reason_counts(&self) -> BTreeMap<ChangeViewReason, usize> {
        self.change_view_reason_counts.clone()
    }

    pub fn change_view_total(&self) -> u64 {
        self.change_view_total
    }

    pub(crate) fn change_view_target(&self) -> Option<ViewNumber> {
        self.records
            .get(&MessageKind::ChangeView)
            .and_then(|messages| messages.first())
            .and_then(|msg| match msg.message {
                ConsensusMessage::ChangeView { new_view, .. } => Some(new_view),
                _ => None,
            })
    }
}

use crate::message::{ConsensusMessage, SignedMessage};

use crate::state::core::ConsensusState;

pub(super) fn record_change_view(state: &mut ConsensusState, message: &SignedMessage) {
    if let ConsensusMessage::ChangeView { reason, .. } = &message.message {
        state.change_view_reasons.insert(message.validator, *reason);
        *state.change_view_reason_counts.entry(*reason).or_insert(0) += 1;
        state.change_view_total = state.change_view_total.saturating_add(1);
    }
}

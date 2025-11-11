use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, MessageKind, SignedMessage},
};

use crate::state::core::ConsensusState;

pub(super) fn validate_height_and_validator(
    state: &ConsensusState,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if message.height != state.height {
        return Err(ConsensusError::InvalidHeight {
            expected: state.height,
            received: message.height,
        });
    }
    if state.validators.get(message.validator).is_none() {
        return Err(ConsensusError::UnknownValidator(message.validator));
    }
    Ok(())
}

pub(super) fn validate_view(
    state: &ConsensusState,
    kind: MessageKind,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    match kind {
        MessageKind::ChangeView => {
            if message.view != state.view {
                return Err(ConsensusError::StaleMessage {
                    kind: MessageKind::ChangeView,
                    current_view: state.view,
                    message_view: message.view,
                });
            }
        }
        _ if message.view != state.view => {
            return Err(ConsensusError::InvalidView {
                expected: state.view,
                received: message.view,
            });
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn ensure_not_duplicate(
    state: &ConsensusState,
    kind: MessageKind,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if let Some(entry) = state.records.get(&kind) {
        if entry.iter().any(|m| m.validator == message.validator) {
            return Err(ConsensusError::DuplicateMessage {
                kind,
                validator: message.validator,
            });
        }
    }
    Ok(())
}

pub(super) fn ensure_change_view_consistency(
    state: &ConsensusState,
    kind: MessageKind,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if let ConsensusMessage::ChangeView { new_view, .. } = &message.message {
        if *new_view <= state.view {
            return Err(ConsensusError::StaleView {
                current: state.view,
                requested: *new_view,
            });
        }
        if let Some(existing) = state.records.get(&kind).and_then(|entry| entry.first()) {
            if let ConsensusMessage::ChangeView {
                new_view: existing_view,
                ..
            } = existing.message
            {
                if *new_view != existing_view {
                    return Err(ConsensusError::InconsistentView {
                        expected: existing_view,
                        received: *new_view,
                    });
                }
            }
        }
    }
    Ok(())
}

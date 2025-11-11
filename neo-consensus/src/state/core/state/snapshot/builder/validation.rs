use hashbrown::HashSet;

use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, MessageKind, SignedMessage},
    validator::{ValidatorId, ValidatorSet},
};

pub(crate) fn validate_message(
    validators: &ValidatorSet,
    height: u64,
    view: crate::message::ViewNumber,
    kind: MessageKind,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if message.kind() != kind {
        return Err(ConsensusError::InvalidView {
            expected: view,
            received: message.view,
        });
    }
    if validators.get(message.validator).is_none() {
        return Err(ConsensusError::UnknownValidator(message.validator));
    }
    if message.height != height {
        return Err(ConsensusError::InvalidHeight {
            expected: height,
            received: message.height,
        });
    }
    match kind {
        MessageKind::ChangeView => {
            if message.view != view {
                return Err(ConsensusError::StaleMessage {
                    kind,
                    current_view: view,
                    message_view: message.view,
                });
            }
        }
        _ => {
            if message.view != view {
                return Err(ConsensusError::InvalidView {
                    expected: view,
                    received: message.view,
                });
            }
        }
    }
    if let ConsensusMessage::PrepareRequest { .. } = message.message {
        let expected = validators
            .primary_id(height, view)
            .ok_or(ConsensusError::NoValidators)?;
        if message.validator != expected {
            return Err(ConsensusError::InvalidPrimary {
                expected,
                actual: message.validator,
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_participation_entry(
    seen: &mut HashSet<ValidatorId>,
    kind: MessageKind,
    validator: ValidatorId,
) -> Result<(), ConsensusError> {
    if !seen.insert(validator) {
        return Err(ConsensusError::DuplicateMessage { kind, validator });
    }
    Ok(())
}

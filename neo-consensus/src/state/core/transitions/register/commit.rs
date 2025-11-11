use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, MessageKind, SignedMessage},
};

use crate::state::core::ConsensusState;

pub(super) fn ensure_commit_ready(
    state: &ConsensusState,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if matches!(message.message, ConsensusMessage::Commit { .. }) {
        let responded = state
            .records
            .get(&MessageKind::PrepareResponse)
            .map(|responses| {
                responses
                    .iter()
                    .any(|entry| entry.validator == message.validator)
            })
            .unwrap_or(false);
        if !responded {
            return Err(ConsensusError::MissingPrepareResponse {
                validator: message.validator,
            });
        }
    }
    Ok(())
}

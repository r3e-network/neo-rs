use crate::{
    error::ConsensusError,
    message::{ConsensusMessage, SignedMessage},
};

use crate::state::core::ConsensusState;

pub(super) fn validate_primary(
    state: &ConsensusState,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if let ConsensusMessage::PrepareRequest { .. } = message.message {
        let expected = state
            .validators
            .primary_id(state.height, state.view)
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

pub(super) fn validate_proposal_hash(
    state: &ConsensusState,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if !matches!(message.message, ConsensusMessage::PrepareRequest { .. }) {
        if let Some(actual_hash) = message.message.proposal_hash() {
            match state.proposal {
                Some(expected) if expected == actual_hash => {}
                Some(expected) => {
                    return Err(ConsensusError::ProposalMismatch {
                        expected,
                        actual: actual_hash,
                    })
                }
                None => {
                    return Err(ConsensusError::MissingProposal);
                }
            }
        }
    }
    Ok(())
}

pub(super) fn record_prepare_request(
    state: &mut ConsensusState,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if let ConsensusMessage::PrepareRequest { proposal_hash, .. } = message.message {
        match state.proposal {
            None => state.proposal = Some(proposal_hash),
            Some(existing) if existing != proposal_hash => {
                return Err(ConsensusError::ProposalMismatch {
                    expected: existing,
                    actual: proposal_hash,
                })
            }
            _ => {}
        }
    }
    Ok(())
}

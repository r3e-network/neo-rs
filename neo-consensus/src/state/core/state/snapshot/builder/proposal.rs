use neo_base::hash::Hash256;

use crate::{error::ConsensusError, message::SignedMessage};

pub(crate) fn validate_proposal(
    proposal: &mut Option<Hash256>,
    message: &SignedMessage,
) -> Result<(), ConsensusError> {
    if let Some(hash) = message.message.proposal_hash() {
        match proposal {
            Some(current) if *current != hash => {
                return Err(ConsensusError::ProposalMismatch {
                    expected: *current,
                    actual: hash,
                })
            }
            None => {
                *proposal = Some(hash);
            }
            _ => {}
        }
    }
    Ok(())
}

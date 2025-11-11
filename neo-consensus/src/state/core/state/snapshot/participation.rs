use hashbrown::{HashMap, HashSet};
use neo_base::hash::Hash256;

use crate::{
    error::ConsensusError,
    message::{MessageKind, SignedMessage},
    validator::ValidatorSet,
};

use super::builder::{validate_message, validate_participation_entry, validate_proposal};

pub(super) fn restore_participation(
    validators: &ValidatorSet,
    height: u64,
    view: crate::message::ViewNumber,
    participation: HashMap<MessageKind, Vec<SignedMessage>>,
    proposal: &mut Option<Hash256>,
) -> Result<HashMap<MessageKind, Vec<SignedMessage>>, ConsensusError> {
    let mut records = HashMap::new();
    for (kind, messages) in participation {
        let mut seen = HashSet::new();
        for message in &messages {
            validate_message(validators, height, view, kind, message)?;
            validate_participation_entry(&mut seen, kind, message.validator)?;
            validate_proposal(proposal, message)?;
        }
        records.insert(kind, messages);
    }
    Ok(records)
}

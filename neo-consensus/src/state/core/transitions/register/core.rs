use crate::{error::ConsensusError, message::SignedMessage};

use super::{
    change_view::record_change_view,
    commit::ensure_commit_ready,
    proposal::{record_prepare_request, validate_primary, validate_proposal_hash},
    validation::{
        ensure_change_view_consistency, ensure_not_duplicate, validate_height_and_validator,
        validate_view,
    },
};

use crate::state::core::ConsensusState;

impl ConsensusState {
    pub fn register_message(&mut self, message: SignedMessage) -> Result<(), ConsensusError> {
        validate_height_and_validator(self, &message)?;
        let kind = message.kind();
        validate_view(self, kind, &message)?;
        ensure_not_duplicate(self, kind, &message)?;
        ensure_change_view_consistency(self, kind, &message)?;
        validate_primary(self, &message)?;
        validate_proposal_hash(self, &message)?;
        record_prepare_request(self, &message)?;
        ensure_commit_ready(self, &message)?;
        record_change_view(self, &message);

        self.records.entry(kind).or_default().push(message);
        self.refresh_expected(kind);
        Ok(())
    }
}

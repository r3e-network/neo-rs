mod builder;
mod participation;
mod validators;

use crate::{error::ConsensusError, state::SnapshotState, validator::ValidatorSet};

use super::ConsensusState;

impl ConsensusState {
    pub fn snapshot(&self) -> SnapshotState {
        SnapshotState::from(self)
    }

    pub fn from_snapshot(
        validators: ValidatorSet,
        snapshot: SnapshotState,
    ) -> Result<Self, ConsensusError> {
        builder::from_snapshot(validators, snapshot)
    }
}

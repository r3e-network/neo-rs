mod proposal;
mod validation;

use hashbrown::HashMap;

use crate::{
    error::ConsensusError,
    state::SnapshotState,
    validator::{ValidatorId, ValidatorSet},
    ConsensusState,
};

use super::{participation::restore_participation, validators::restore_expected_validators};

pub(super) fn from_snapshot(
    validators: ValidatorSet,
    snapshot: SnapshotState,
) -> Result<ConsensusState, ConsensusError> {
    let SnapshotState {
        height,
        view,
        proposal,
        participation,
        expected,
        change_view_reasons,
        change_view_reason_counts,
        change_view_total,
    } = snapshot;

    let mut proposal = proposal;
    let records = restore_participation(
        &validators,
        height,
        view,
        participation.into_iter().collect(),
        &mut proposal,
    )?;
    let expected_map = restore_expected_validators(&validators, expected.into_iter().collect())?;
    let change_view_reasons_map: HashMap<ValidatorId, _> =
        change_view_reasons.into_iter().collect();

    let mut state = ConsensusState {
        height,
        view,
        validators,
        records,
        proposal,
        expected: expected_map,
        change_view_reasons: change_view_reasons_map,
        change_view_reason_counts,
        change_view_total,
    };
    state.seed_prepare_request_expectation();
    Ok(state)
}

pub(super) use proposal::validate_proposal;
pub(super) use validation::{validate_message, validate_participation_entry};

use super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ConsensusMessage, ViewNumber},
    state::ConsensusState,
    validator::ValidatorId,
    ConsensusError, DbftEngine, QuorumDecision,
};
use neo_base::hash::Hash256;

#[test]
fn prepare_response_requires_registered_proposal() {
    let (set, privs) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set);
    let mut engine = DbftEngine::new(state);

    let response = build_signed(
        &privs[1],
        1,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: Hash256::new([0x55; 32]),
        },
    );
    let err = engine.process_message(response).unwrap_err();
    assert!(matches!(err, ConsensusError::MissingProposal));
}

#[test]
fn prepare_request_must_come_from_primary() {
    let (set, privs) = generate_validators();
    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let non_primary_index = (primary_index + 1) % set.len();
    let non_primary_id = ValidatorId(non_primary_index as u16);

    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x21; 32]);

    let bad = build_signed(
        &privs[non_primary_index],
        non_primary_id.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    let err = engine.process_message(bad).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::InvalidPrimary {
            expected,
            actual
        } if expected == primary && actual == non_primary_id
    ));

    let good = build_signed(
        &privs[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    matches!(
        engine.process_message(good).unwrap(),
        QuorumDecision::Pending
    );
}

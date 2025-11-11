use super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ConsensusMessage, ViewNumber},
    state::ConsensusState,
    validator::ValidatorId,
    ConsensusError, DbftEngine, QuorumDecision,
};
use neo_base::hash::Hash256;

#[test]
fn commit_requires_registered_proposal() {
    let (set, privs) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);

    let commit = build_signed(
        &privs[0],
        0,
        ViewNumber::ZERO,
        ConsensusMessage::Commit {
            proposal_hash: Hash256::new([0x44; 32]),
        },
    );
    let err = engine.process_message(commit).unwrap_err();
    assert!(matches!(err, ConsensusError::MissingProposal));
}

#[test]
fn commit_requires_prepare_response_from_validator() {
    let (set, privs) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x99; 32]);

    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let request = build_signed(
        &privs[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    engine.process_message(request).unwrap();

    let other_index = (0..set.len()).find(|idx| *idx != primary_index).unwrap();
    let commit = build_signed(
        &privs[other_index],
        other_index as u16,
        ViewNumber::ZERO,
        ConsensusMessage::Commit {
            proposal_hash: proposal,
        },
    );
    let err = engine.process_message(commit).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::MissingPrepareResponse { validator }
            if validator == ValidatorId(other_index as u16)
    ));

    let response = build_signed(
        &privs[other_index],
        other_index as u16,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    engine.process_message(response).unwrap();

    let commit = build_signed(
        &privs[other_index],
        other_index as u16,
        ViewNumber::ZERO,
        ConsensusMessage::Commit {
            proposal_hash: proposal,
        },
    );
    matches!(
        engine.process_message(commit).unwrap(),
        QuorumDecision::Pending
    );
}

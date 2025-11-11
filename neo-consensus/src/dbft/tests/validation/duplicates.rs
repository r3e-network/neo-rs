use super::super::helpers::*;
use super::super::*;

#[test]
fn duplicate_prepare_response_rejected() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x22; 32]);

    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let request = build_signed(
        &priv_keys[primary_index],
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
    let response = build_signed(
        &priv_keys[other_index],
        other_index as u16,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    engine.process_message(response.clone()).unwrap();
    let err = engine.process_message(response).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::DuplicateMessage {
            kind: MessageKind::PrepareResponse,
            validator
        } if validator == ValidatorId(other_index as u16)
    ));
}

#[test]
fn duplicate_commit_rejected() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x77; 32]);

    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let request = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareRequest {
            proposal_hash: proposal,
            height: HEIGHT,
            tx_hashes: vec![],
        },
    );
    engine.process_message(request).unwrap();

    let response = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    engine.process_message(response).unwrap();

    let commit = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::Commit {
            proposal_hash: proposal,
        },
    );
    engine.process_message(commit.clone()).unwrap();
    let err = engine.process_message(commit).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::DuplicateMessage {
            kind: MessageKind::Commit,
            validator
        } if validator == primary
    ));
}

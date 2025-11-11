use super::helpers::*;
use super::*;

#[test]
fn snapshot_roundtrip_preserves_participation() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x33; 32]);

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

    let snapshot = engine.snapshot();
    let mut bytes = Vec::new();
    snapshot.neo_encode(&mut bytes);
    let mut reader = SliceReader::new(bytes.as_slice());
    let decoded = SnapshotState::neo_decode(&mut reader).unwrap();
    assert_eq!(snapshot, decoded);
    let mut restored = DbftEngine::from_snapshot(set, decoded).unwrap();
    let err = restored.process_message(response).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::DuplicateMessage {
            kind: MessageKind::PrepareResponse,
            validator
        } if validator == ValidatorId(other_index as u16)
    ));
    assert_eq!(
        engine.expected_participants(MessageKind::Commit),
        restored.expected_participants(MessageKind::Commit)
    );
}

#[test]
fn snapshot_restores_prepare_request_expectation() {
    let (set, _privs) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let engine = DbftEngine::new(state);
    let snapshot = engine.snapshot();
    let restored = DbftEngine::from_snapshot(set.clone(), snapshot).unwrap();

    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    assert_eq!(
        restored.expected_participants(MessageKind::PrepareRequest),
        Some(vec![primary])
    );
    assert_eq!(
        restored.missing_validators(MessageKind::PrepareRequest),
        vec![primary]
    );
}

#[test]
fn advance_height_resets_state() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x44; 32]);

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

    let err = engine.advance_height(HEIGHT).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::InvalidHeightTransition { .. }
    ));

    engine.advance_height(HEIGHT + 1).unwrap();
    assert_eq!(engine.state().height(), HEIGHT + 1);
    assert_eq!(engine.state().view(), ViewNumber::ZERO);
    assert!(engine.state().proposal().is_none());
    assert_eq!(engine.state().tally(MessageKind::PrepareResponse), 0);
}

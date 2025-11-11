use super::super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ConsensusMessage, MessageKind},
    state::ConsensusState,
    DbftEngine, ViewNumber,
};
use neo_base::hash::Hash256;

#[test]
fn commit_missing_is_deferred_until_stage_active() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let proposal = Hash256::new([0x42; 32]);

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

    let primary_response = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::PrepareResponse {
            proposal_hash: proposal,
        },
    );
    engine.process_message(primary_response).unwrap();

    assert_eq!(
        engine.missing_validators(MessageKind::Commit),
        vec![primary]
    );

    let commit = build_signed(
        &priv_keys[primary_index],
        primary.0,
        ViewNumber::ZERO,
        ConsensusMessage::Commit {
            proposal_hash: proposal,
        },
    );
    engine.process_message(commit).unwrap();

    assert!(engine.missing_validators(MessageKind::Commit).is_empty());
    assert!(engine.expected_participants(MessageKind::Commit).is_none());
}

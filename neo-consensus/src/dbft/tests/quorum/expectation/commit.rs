use super::super::super::{helpers::*, HEIGHT};
use crate::validator::ValidatorId;
use crate::{
    message::{ConsensusMessage, MessageKind, ViewNumber},
    state::ConsensusState,
    DbftEngine,
};
use neo_base::hash::Hash256;

#[test]
fn expected_participants_for_commit_follow_prepare_responses() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);

    assert!(engine.expected_participants(MessageKind::Commit).is_none());

    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    let primary_index = set.index_of(primary).unwrap();
    let proposal = Hash256::new([0x31; 32]);
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

    let responders = [primary_index, (primary_index + 1) % set.len()];
    for idx in responders.iter().copied() {
        let response = build_signed(
            &priv_keys[idx],
            idx as u16,
            ViewNumber::ZERO,
            ConsensusMessage::PrepareResponse {
                proposal_hash: proposal,
            },
        );
        engine.process_message(response).unwrap();
    }

    let expected = responders
        .iter()
        .map(|idx| ValidatorId(*idx as u16))
        .collect::<Vec<_>>();
    assert_eq!(
        engine.expected_participants(MessageKind::Commit).unwrap(),
        expected
    );

    for idx in responders.iter().copied() {
        let commit = build_signed(
            &priv_keys[idx],
            idx as u16,
            ViewNumber::ZERO,
            ConsensusMessage::Commit {
                proposal_hash: proposal,
            },
        );
        engine.process_message(commit).unwrap();
    }

    assert!(engine.expected_participants(MessageKind::Commit).is_none());
}

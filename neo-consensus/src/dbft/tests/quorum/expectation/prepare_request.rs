use super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ChangeViewReason, MessageKind, ViewNumber},
    state::ConsensusState,
    DbftEngine, QuorumDecision,
};

#[test]
fn prepare_request_expectation_tracks_primary() {
    let (set, _) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let engine = DbftEngine::new(state);
    let primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    assert_eq!(
        engine.expected_participants(MessageKind::PrepareRequest),
        Some(vec![primary])
    );
}

#[test]
fn prepare_request_expectation_updates_with_view_and_height() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let initial_primary = set.primary_id(HEIGHT, ViewNumber::ZERO).unwrap();
    assert_eq!(
        engine.expected_participants(MessageKind::PrepareRequest),
        Some(vec![initial_primary])
    );

    let target_view = ViewNumber(1);
    for idx in 0..set.quorum() {
        let change = build_signed(
            &priv_keys[idx],
            idx as u16,
            ViewNumber::ZERO,
            change_view_msg(target_view, ChangeViewReason::Timeout),
        );
        let decision = engine.process_message(change).unwrap();
        if idx + 1 == set.quorum() {
            assert!(matches!(
                decision,
                QuorumDecision::ViewChange { new_view, .. } if new_view == target_view
            ));
        }
    }

    let new_primary = set.primary_id(HEIGHT, target_view).unwrap();
    assert_eq!(
        engine.expected_participants(MessageKind::PrepareRequest),
        Some(vec![new_primary])
    );

    let next_height = HEIGHT + 1;
    engine.advance_height(next_height).unwrap();
    let height_primary = set.primary_id(next_height, ViewNumber::ZERO).unwrap();
    assert_eq!(
        engine.expected_participants(MessageKind::PrepareRequest),
        Some(vec![height_primary])
    );
}

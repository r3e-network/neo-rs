use super::super::super::helpers::*;
use super::super::super::*;

#[test]
fn change_view_quorum_advances_state() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);

    let target_view = ViewNumber(1);
    let participants: Vec<usize> = (0..set.len()).collect();
    for idx in participants.iter().copied().take(set.quorum() - 1) {
        let change = build_signed(
            &priv_keys[idx],
            idx as u16,
            ViewNumber::ZERO,
            change_view_msg(target_view, ChangeViewReason::Timeout),
        );
        engine.process_message(change).unwrap();
    }
    assert_eq!(engine.state().view(), ViewNumber::ZERO);

    let change = build_signed(
        &priv_keys[participants[set.quorum() - 1]],
        participants[set.quorum() - 1] as u16,
        ViewNumber::ZERO,
        change_view_msg(target_view, ChangeViewReason::Timeout),
    );
    matches!(
        engine.process_message(change).unwrap(),
        QuorumDecision::ViewChange { new_view, .. } if new_view == target_view
    );
    assert_eq!(engine.state().view(), target_view);
}

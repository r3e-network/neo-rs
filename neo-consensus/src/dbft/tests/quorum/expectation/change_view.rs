use super::super::super::{helpers::*, HEIGHT};
use crate::{
    message::{ChangeViewReason, MessageKind, ViewNumber},
    state::ConsensusState,
    DbftEngine,
};

#[test]
fn expected_participants_for_change_view_activate_with_messages() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    assert!(engine
        .expected_participants(MessageKind::ChangeView)
        .is_none());

    let change = build_signed(
        &priv_keys[0],
        0,
        ViewNumber::ZERO,
        change_view_msg(ViewNumber(1), ChangeViewReason::Timeout),
    );
    engine.process_message(change).unwrap();

    let expected = set.iter().map(|validator| validator.id).collect::<Vec<_>>();
    assert_eq!(
        engine
            .expected_participants(MessageKind::ChangeView)
            .unwrap(),
        expected
    );
}

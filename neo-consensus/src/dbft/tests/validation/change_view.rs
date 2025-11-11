use super::super::helpers::*;
use super::super::*;

#[test]
fn change_view_reasons_tracked_and_restored() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let target_view = ViewNumber(1);

    let changes = [
        (0u16, ChangeViewReason::Timeout),
        (2u16, ChangeViewReason::TxInvalid),
    ];

    for (validator, reason) in changes {
        let change = build_signed(
            &priv_keys[validator as usize],
            validator,
            ViewNumber::ZERO,
            change_view_msg(target_view, reason),
        );
        engine.process_message(change).unwrap();
    }

    let reasons = engine.change_view_reasons();
    assert_eq!(
        reasons.keys().copied().collect::<Vec<ValidatorId>>(),
        vec![ValidatorId(0), ValidatorId(2)]
    );
    assert_eq!(
        reasons.get(&ValidatorId(0)),
        Some(&ChangeViewReason::Timeout)
    );
    assert_eq!(
        reasons.get(&ValidatorId(2)),
        Some(&ChangeViewReason::TxInvalid)
    );
    let counts = engine.change_view_reason_counts();
    assert_eq!(counts.get(&ChangeViewReason::Timeout), Some(&1usize));
    assert_eq!(counts.get(&ChangeViewReason::TxInvalid), Some(&1usize));
    assert_eq!(engine.change_view_total(), 2);

    let snapshot = engine.snapshot();
    assert_eq!(snapshot.change_view_reasons.len(), 2);
    assert_eq!(
        snapshot
            .change_view_reasons
            .get(&ValidatorId(0))
            .copied()
            .unwrap(),
        ChangeViewReason::Timeout
    );
    assert_eq!(
        snapshot
            .change_view_reason_counts
            .get(&ChangeViewReason::Timeout),
        Some(&1usize)
    );
    assert_eq!(snapshot.change_view_total, 2);

    let mut restored = DbftEngine::from_snapshot(set.clone(), snapshot).unwrap();
    assert_eq!(restored.change_view_reasons(), reasons);
    assert_eq!(restored.change_view_reason_counts(), counts);
    assert_eq!(restored.change_view_total(), 2);

    let trigger = build_signed(
        &priv_keys[1],
        1,
        ViewNumber::ZERO,
        change_view_msg(target_view, ChangeViewReason::ChangeAgreement),
    );
    let decision = restored.process_message(trigger).unwrap();
    assert!(matches!(
        decision,
        QuorumDecision::ViewChange { new_view, .. } if new_view == target_view
    ));
    assert!(restored.change_view_reasons().is_empty());
}

#[test]
fn change_view_rejects_previous_view_messages() {
    let (set, priv_keys) = generate_validators();
    let state = ConsensusState::new(HEIGHT, ViewNumber::ZERO, set.clone());
    let mut engine = DbftEngine::new(state);
    let target_view = ViewNumber(1);

    for idx in 0..set.quorum() {
        let change = build_signed(
            &priv_keys[idx],
            idx as u16,
            ViewNumber::ZERO,
            change_view_msg(target_view, ChangeViewReason::Timeout),
        );
        engine.process_message(change).unwrap();
    }
    assert_eq!(engine.state().view(), target_view);

    let stale = build_signed(
        &priv_keys[set.quorum()],
        set.quorum() as u16,
        ViewNumber::ZERO,
        change_view_msg(ViewNumber(2), ChangeViewReason::Timeout),
    );
    let err = engine.process_message(stale).unwrap_err();
    assert!(matches!(
        err,
        ConsensusError::StaleMessage {
            kind: MessageKind::ChangeView,
            ..
        }
    ));
}

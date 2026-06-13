use super::helpers::{
    create_test_validators, create_validators_with_keys, sign_commit, sign_payload,
};
use crate::messages::{
    ChangeViewMessage, CommitMessage, ConsensusPayload, PrepareRequestMessage,
    PrepareResponseMessage,
};
use crate::{ChangeViewReason, ConsensusMessageType};
use crate::{ConsensusEvent, ConsensusService};
use neo_primitives::UInt256;
use tokio::sync::mpsc;

#[tokio::test]
async fn timer_tick_triggers_change_view_broadcast() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(1), vec![], tx);

    service.start(0, 0, UInt256::zero(), 0).unwrap();
    service
        .on_timer_tick(crate::context::BLOCK_TIME_MS * 2 + 1)
        .unwrap();

    let mut change_view = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::ChangeView {
                change_view = Some(payload);
                break;
            }
        }
    }

    let payload = change_view.expect("change view payload");
    let msg = ChangeViewMessage::deserialize(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    )
    .expect("change view deserialize");
    assert_eq!(msg.reason, ChangeViewReason::Timeout);
    assert_eq!(msg.new_view_number().unwrap(), 1);
}

#[tokio::test]
async fn timeout_change_view_is_not_rebroadcast_before_expected_view_timer() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(1), vec![], tx);
    service.set_expected_block_time(1_000);

    service.start(0, 10_000, UInt256::zero(), 0).unwrap();
    for validator_index in 0..4 {
        service.context.update_last_seen_message(validator_index, 0);
    }

    service.on_timer_tick(12_000).unwrap();
    let event = rx.try_recv().expect("first change view");
    assert!(matches!(
        event,
        ConsensusEvent::BroadcastMessage(payload)
            if payload.message_type == ConsensusMessageType::ChangeView
    ));

    service.on_timer_tick(12_999).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "C# reschedules RequestChangeView by TimePerBlock << (expectedView + 1)"
    );

    service.on_timer_tick(16_000).unwrap();
    let event = rx.try_recv().expect("second change view after reschedule");
    assert!(matches!(
        event,
        ConsensusEvent::BroadcastMessage(payload)
            if payload.message_type == ConsensusMessageType::ChangeView
    ));
}

#[tokio::test]
async fn primary_requests_transactions_at_prepare_timer_once() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);
    service.set_expected_block_time(1_000);

    service.start(0, 10_000, UInt256::zero(), 0).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "C# schedules the primary PrepareRequest instead of requesting txs immediately"
    );

    service.on_timer_tick(10_999).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "timer must not fire before the TimePerBlock delay"
    );

    service.on_timer_tick(11_000).unwrap();
    let event = rx.try_recv().expect("request transactions at deadline");
    assert!(matches!(
        event,
        ConsensusEvent::RequestTransactions { block_index: 0, .. }
    ));

    service.on_timer_tick(11_500).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "a split request/response architecture must not spam mempool requests"
    );

    service.on_timer_tick(12_000).unwrap();
    let event = rx
        .try_recv()
        .expect("change view when transaction response never arrives");
    assert!(matches!(
        event,
        ConsensusEvent::BroadcastMessage(payload)
            if payload.message_type == ConsensusMessageType::ChangeView
    ));
}

#[tokio::test]
async fn late_primary_prepare_timer_extends_follow_up_timeout() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);
    service.set_expected_block_time(1_000);

    service.start(0, 10_000, UInt256::zero(), 0).unwrap();
    service.on_timer_tick(11_250).unwrap();
    let event = rx.try_recv().expect("late prepare timer requests txs");
    assert!(matches!(
        event,
        ConsensusEvent::RequestTransactions { block_index: 0, .. }
    ));

    service.on_timer_tick(12_000).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "follow-up timeout is relative to the actual timer fire"
    );

    service.on_timer_tick(12_250).unwrap();
    let event = rx
        .try_recv()
        .expect("late prepare timer follow-up eventually changes view");
    assert!(matches!(
        event,
        ConsensusEvent::BroadcastMessage(payload)
            if payload.message_type == ConsensusMessageType::ChangeView
    ));
}

#[tokio::test]
async fn timer_after_commit_resends_recovery_message_instead_of_change_view() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);

    service.start(0, 10_000, UInt256::zero(), 0).unwrap();
    service
        .context
        .add_commit(1, 0, vec![0x42; 64])
        .expect("local commit");

    service.on_timer_tick(12_000).unwrap();

    let mut recovery_message = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            match payload.message_type {
                ConsensusMessageType::RecoveryMessage => {
                    recovery_message = Some(payload);
                    break;
                }
                ConsensusMessageType::ChangeView => {
                    panic!("C# resends RecoveryMessage after CommitSent instead of ChangeView")
                }
                _ => {}
            }
        }
    }

    let payload = recovery_message.expect("recovery message payload");
    assert_eq!(payload.validator_index, 1);
    assert_eq!(payload.view_number, 0);
    assert!(service.context().change_views.is_empty());

    service.on_timer_tick(12_999).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "commit recovery resend should follow the C# TimePerBlock << 1 delay"
    );

    service.on_timer_tick(14_000).unwrap();
    let event = rx
        .try_recv()
        .expect("periodic commit recovery resend payload");
    assert!(matches!(
        event,
        ConsensusEvent::BroadcastMessage(payload)
            if payload.message_type == ConsensusMessageType::RecoveryMessage
    ));
}

#[tokio::test]
async fn backup_timeout_with_missing_transactions_uses_tx_not_found_reason() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);

    service.start(0, 10_000, UInt256::zero(), 0).unwrap();
    for validator_index in 0..4 {
        service.context.update_last_seen_message(validator_index, 0);
    }

    let tx_hashes = vec![UInt256::from([0x10; 32]), UInt256::from([0x20; 32])];
    let prepare =
        PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 10_001, 42, tx_hashes.clone());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    service.process_message(payload).unwrap();
    service
        .on_transactions_received(vec![tx_hashes[0]])
        .expect("partial transaction set");
    while rx.try_recv().is_ok() {}

    service.on_timer_tick(12_000).unwrap();

    let mut change_view = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::ChangeView {
                change_view = Some(payload);
                break;
            }
        }
    }

    let payload = change_view.expect("change view payload");
    let msg = ChangeViewMessage::deserialize(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    )
    .expect("change view deserialize");
    assert_eq!(msg.reason, ChangeViewReason::TxNotFound);
}

#[tokio::test]
async fn view_change_rotates_primary_by_view() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service.start(10, 0, UInt256::zero(), 0).unwrap();
    assert_eq!(service.context().primary_index(), 3);

    service
        .context
        .reset_for_new_view(1, crate::context::BLOCK_TIME_MS * 2 + 1);
    assert_eq!(service.context().primary_index(), 2);

    service
        .context
        .reset_for_new_view(2, crate::context::BLOCK_TIME_MS * 2 + 2);
    assert_eq!(service.context().primary_index(), 1);
}

#[tokio::test]
async fn timeout_view_change_allows_new_prepare_request() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 0, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    service
        .on_timer_tick(crate::context::BLOCK_TIME_MS)
        .unwrap();
    let tx_request = rx.try_recv().expect("initial transaction request");
    assert!(matches!(
        tx_request,
        ConsensusEvent::RequestTransactions { block_index: 0, .. }
    ));

    service
        .on_timer_tick(crate::context::BLOCK_TIME_MS * 2 + 1)
        .unwrap();

    let mut change_view = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::ChangeView {
                change_view = Some(payload);
                break;
            }
        }
    }

    let change_view = change_view.expect("change view payload");
    let change_view_msg = ChangeViewMessage::deserialize(
        &change_view.data,
        change_view.block_index,
        change_view.view_number,
        change_view.validator_index,
    )
    .expect("change view message");
    let new_view = change_view_msg.new_view_number().unwrap();

    let validator_count = service.context.validator_count() as i64;
    let new_primary =
        (service.context.block_index as i64 - new_view as i64).rem_euclid(validator_count) as u8;
    service.context.my_index = Some(new_primary);
    service
        .context
        .reset_for_new_view(new_view, change_view_msg.timestamp);
    while rx.try_recv().is_ok() {}

    let prepare_deadline = service
        .context()
        .view_start_time
        .saturating_add(service.context().prepare_request_delay());
    service.on_timer_tick(prepare_deadline).unwrap();
    let tx_request = rx
        .try_recv()
        .expect("new primary requests transactions at timer");
    assert!(matches!(
        tx_request,
        ConsensusEvent::RequestTransactions { block_index: 0, .. }
    ));

    let tx_hashes = vec![UInt256::from([0x33; 32])];
    service.on_transactions_received(tx_hashes.clone()).unwrap();

    let mut prepare_payload = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                prepare_payload = Some(payload);
                break;
            }
        }
    }

    let prepare_payload = prepare_payload.expect("prepare request");
    let prepare_msg = PrepareRequestMessage::deserialize_body(
        &prepare_payload.data,
        prepare_payload.block_index,
        prepare_payload.view_number,
        prepare_payload.validator_index,
    )
    .expect("prepare request msg");
    assert_eq!(prepare_msg.view_number, new_view);
    assert_eq!(prepare_msg.transaction_hashes, tx_hashes);
}

#[tokio::test]
async fn view_change_allows_consensus_to_complete() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(3), keys[3].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    for validator_index in 0..=2 {
        let msg = ChangeViewMessage::new(
            0,
            0,
            validator_index,
            2_000 + validator_index as u64,
            ChangeViewReason::Timeout,
        );
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::ChangeView,
            msg.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    assert_eq!(service.context().view_number, 1);
    assert!(service.context().is_primary());
    while rx.try_recv().is_ok() {}

    let prepare_deadline = service
        .context()
        .view_start_time
        .saturating_add(service.context().prepare_request_delay());
    service.on_timer_tick(prepare_deadline).unwrap();

    let mut requested = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::RequestTransactions { block_index, .. } = event {
            requested = block_index == 0;
            break;
        }
    }

    assert!(requested);

    let tx_hashes = vec![UInt256::from([0x44; 32])];
    service.on_transactions_received(tx_hashes.clone()).unwrap();

    let mut prepare_payload = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                prepare_payload = Some(payload);
                break;
            }
        }
    }

    let prepare_payload = prepare_payload.expect("prepare request payload");
    let prep_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    for validator_index in 0..=1 {
        let response = PrepareResponseMessage::new(0, 1, validator_index, prep_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            1,
            ConsensusMessageType::PrepareResponse,
            response.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    let mut commit_payload = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::Commit {
                commit_payload = Some(payload);
                break;
            }
        }
    }

    let commit_payload = commit_payload.expect("commit payload");
    assert_eq!(commit_payload.validator_index, 3);
    assert_eq!(commit_payload.view_number, 1);
    assert_eq!(commit_payload.block_index, prepare_payload.block_index);

    let block_hash = service.context().proposed_block_hash.expect("block hash");

    for validator_index in 0..=1 {
        let signature = sign_commit(network, &block_hash, &keys[validator_index as usize]);
        let commit = CommitMessage::new(0, 1, validator_index, signature.clone());
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            1,
            ConsensusMessageType::Commit,
            commit.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert_eq!(committed, Some(0));
}

#[tokio::test]
async fn change_view_threshold_triggers_view_change() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    for validator_index in 1..=3 {
        let msg = ChangeViewMessage::new(
            0,
            0,
            validator_index,
            1_000 + validator_index as u64,
            ChangeViewReason::Timeout,
        );
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::ChangeView,
            msg.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    let mut view_changed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::ViewChanged {
            old_view, new_view, ..
        } = event
        {
            view_changed = Some((old_view, new_view));
            break;
        }
    }

    assert_eq!(view_changed, Some((0, 1)));
    assert_eq!(service.context().view_number, 1);
}

#[tokio::test]
async fn recovery_request_when_more_than_f_committed() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(3), keys[3].to_vec(), tx);
    service.set_expected_block_time(1_000);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    // Simulate 2 nodes committed (F=1 for 4 validators, so 2 > F)
    let block_hash = UInt256::from([0x55; 32]);
    service.context.proposed_block_hash = Some(block_hash);
    for validator_index in 1..=2 {
        let signature = sign_commit(network, &block_hash, &keys[validator_index as usize]);
        let commit = CommitMessage::new(0, 0, validator_index, signature);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::Commit,
            commit.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    service
        .request_change_view(ChangeViewReason::Timeout, 2_000)
        .unwrap();

    let mut recovery_sent = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::RecoveryRequest {
                recovery_sent = true;
                break;
            }
        }
    }

    assert!(recovery_sent);

    service.on_timer_tick(2_999).unwrap();
    assert!(
        rx.try_recv().is_err(),
        "C# RequestChangeView reschedules before falling back to recovery"
    );

    service.on_timer_tick(6_000).unwrap();
    let mut recovery_sent = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::RecoveryRequest {
                recovery_sent = true;
                break;
            }
        }
    }

    assert!(recovery_sent);
}

#[tokio::test]
async fn no_view_change_after_commit_sent() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let block_hash = UInt256::from([0x66; 32]);
    service.context.proposed_block_hash = Some(block_hash);
    let signature = sign_commit(network, &block_hash, &keys[0]);
    service.context.commits.insert(0, signature);

    let msg = ChangeViewMessage::new(0, 0, 1, 2_000, ChangeViewReason::Timeout);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::ChangeView,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert_eq!(service.context.view_number, 0);
    assert!(service.context.change_views.is_empty());
}

#[tokio::test]
async fn m_minus_one_change_views_not_enough() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    // M = 4 - 1 = 3, send only 2 ChangeView messages (M-1)
    for validator_index in 1..=2 {
        let msg = ChangeViewMessage::new(
            0,
            0,
            validator_index,
            1_000 + validator_index as u64,
            ChangeViewReason::Timeout,
        );
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::ChangeView,
            msg.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    assert_eq!(service.context.view_number, 0);
    assert_eq!(service.context.change_views.len(), 2);
}

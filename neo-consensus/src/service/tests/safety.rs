//! Regression tests for dBFT safety bugs CON-1..CON-4.

use super::helpers::{create_validators_with_keys, sign_commit, sign_payload};
use crate::context::{ConsensusState, MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK};
use crate::messages::{ChangeViewMessage, CommitMessage, ConsensusPayload};
use crate::{
    ChangeViewReason, ConsensusError, ConsensusEvent, ConsensusMessageType, ConsensusService,
};
use neo_primitives::UInt256;
use tokio::sync::mpsc;

fn hash_with_tag(tag: u8, index: u64) -> UInt256 {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&index.to_le_bytes());
    bytes[31] = tag;
    UInt256::from(bytes)
}

/// CON-1: a `BlockCommitted` publication that fails because the event channel
/// is full must not mark the round committed. The finalization stays pending and
/// a later trigger republishes it.
#[tokio::test]
async fn failed_finalization_publication_is_retried() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(1);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx.clone());

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

    // Saturate the bounded event channel so publishing BlockCommitted fails.
    tx.try_send(ConsensusEvent::ViewChanged {
        block_index: 0,
        old_view: 0,
        new_view: 1,
    })
    .expect("channel should have room for one event");

    let block_hash = UInt256::from([0x77; 32]);
    service.context.proposed_block_hash = Some(block_hash);
    let mut last_error = None;
    for validator_index in 1..=3 {
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
        if let Err(err) = service.process_message(payload) {
            last_error = Some(err);
        }
    }

    assert!(
        matches!(last_error, Some(ConsensusError::ChannelSendError(_))),
        "finalization publication should have failed, got {last_error:?}"
    );
    assert_ne!(service.context().state, ConsensusState::Committed);
    assert!(service.finalization_pending);
    assert!(service.is_running());

    // A decided-but-unpublished round must not be replaced by a view change,
    // even though this node holds no Commit of its own.
    for validator_index in 1..=3 {
        let msg = ChangeViewMessage::new(0, 0, validator_index, 1_500, ChangeViewReason::Timeout);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::ChangeView,
            msg.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service
            .process_message(payload)
            .expect("ChangeView should be absorbed while finalization is pending");
    }
    assert_eq!(service.context().view_number, 0);
    assert!(service.finalization_pending);

    // Draining the channel lets the pending finalization be retried without
    // advancing the view.
    while rx.try_recv().is_ok() {}
    service
        .on_timer_tick(1_000)
        .expect("retry of the pending finalization should succeed");

    let mut committed = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = true;
            assert_eq!(block_index, 0);
        }
    }
    assert!(committed, "BlockCommitted should be republished");
    assert_eq!(service.context().state, ConsensusState::Committed);
    assert!(!service.finalization_pending);
    assert!(!service.is_running());
    assert_eq!(service.context().view_number, 0);
}

/// CON-2: a validator's rejected hashes are budgeted per block, so a stream of
/// fresh hashes cannot grow the round state without bound.
#[tokio::test]
async fn rejected_hash_budget_caps_per_validator() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(64);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let offered = MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK + 500;
    let hashes: Vec<UInt256> = (0..offered as u64)
        .map(|i| hash_with_tag(0xA5, i))
        .collect();
    let msg = ChangeViewMessage::new(0, 0, 1, 1_500, ChangeViewReason::TxInvalid)
        .with_rejected_hashes(hashes);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::ChangeView,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);
    service
        .process_message(payload)
        .expect("first ChangeView should be accepted");

    assert_eq!(
        service.context().rejected_tx_validators.len(),
        MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK
    );

    // A later ChangeView from the same validator offering fresh hashes is
    // still accepted (it advances the view) but adds nothing beyond the budget.
    let fresh: Vec<UInt256> = (0..10u64).map(|i| hash_with_tag(0xB6, i)).collect();
    let msg = ChangeViewMessage::new(0, 1, 1, 1_600, ChangeViewReason::TxInvalid)
        .with_rejected_hashes(fresh);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        1,
        ConsensusMessageType::ChangeView,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);
    service
        .process_message(payload)
        .expect("second ChangeView should be accepted");

    assert_eq!(
        service.context().rejected_tx_validators.len(),
        MAX_REJECTED_HASHES_PER_VALIDATOR_PER_BLOCK
    );
    assert_eq!(service.context().view_number, 0);
}

/// CON-2: the stale-view gate runs before rejected-hash accounting.
#[tokio::test]
async fn stale_change_view_records_no_rejected_hashes() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(64);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(2), keys[2].to_vec(), tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");
    service.context.reset_for_new_view(1, 2_000);

    let hashes = vec![hash_with_tag(0x81, 0), hash_with_tag(0x82, 1)];
    let msg = ChangeViewMessage::new(0, 0, 1, 1_500, ChangeViewReason::TxInvalid)
        .with_rejected_hashes(hashes);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::ChangeView,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);
    service
        .process_message(payload)
        .expect("stale ChangeView is answered, not rejected");

    assert!(
        service.context().rejected_tx_validators.is_empty(),
        "a stale ChangeView must not contribute rejected hashes"
    );
    assert!(service.context().change_views.is_empty());
}

/// CON-2: the commit-sent gate runs before rejected-hash accounting.
#[tokio::test]
async fn committed_node_records_no_rejected_hashes() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(64);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let block_hash = UInt256::from([0x66; 32]);
    service.context.proposed_block_hash = Some(block_hash);
    let signature = sign_commit(network, &block_hash, &keys[0]);
    service.context.commits.insert(0, signature);
    service.context.commit_view_numbers.insert(0, 0);

    let msg = ChangeViewMessage::new(0, 0, 1, 2_000, ChangeViewReason::TxInvalid)
        .with_rejected_hashes(vec![hash_with_tag(0x91, 0)]);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::ChangeView,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);
    service
        .process_message(payload)
        .expect("ChangeView is ignored, not rejected");

    assert!(service.context().rejected_tx_validators.is_empty());
    assert!(service.context().change_views.is_empty());
}

/// CON-3: the public view-change entry point honours the local commit lock.
#[tokio::test]
async fn request_change_view_ignored_after_local_commit() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(64);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let block_hash = UInt256::from([0x66; 32]);
    service.context.proposed_block_hash = Some(block_hash);
    let signature = sign_commit(network, &block_hash, &keys[0]);
    service.context.commits.insert(0, signature);
    service.context.commit_view_numbers.insert(0, 0);

    service
        .request_change_view(ChangeViewReason::Timeout, 2_000)
        .expect("view change request should be ignored, not fail");

    assert_eq!(service.context().view_number, 0);
    assert!(service.context().change_views.is_empty());
    let change_views = std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|event| {
            matches!(event, ConsensusEvent::BroadcastMessage(payload) if payload.message_type == ConsensusMessageType::ChangeView)
        })
        .count();
    assert_eq!(change_views, 0);
}

/// CON-4: with a single validator the primary's own `PrepareRequest` already
/// satisfies the preparation quorum (N=1, M=1), so the round must finalize
/// without waiting for a `PrepareResponse` that can never arrive.
#[tokio::test]
async fn single_validator_round_finalizes_block() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(64);
    let (validators, keys) = create_validators_with_keys(1);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(service.context().is_primary());

    let deadline = service.context().view_start_time + service.context().get_timeout();
    service
        .on_timer_tick(deadline.saturating_add(1))
        .expect("primary timer should start the round");
    service
        .on_transactions_received(Vec::new())
        .expect("empty proposal should be installed");

    let mut committed = None;
    let mut commit_broadcast = false;
    while let Ok(event) = rx.try_recv() {
        match event {
            ConsensusEvent::BlockCommitted { block_index, .. } => committed = Some(block_index),
            ConsensusEvent::BroadcastMessage(payload)
                if payload.message_type == ConsensusMessageType::Commit =>
            {
                commit_broadcast = true;
            }
            _ => {}
        }
    }

    assert!(commit_broadcast, "the primary should sign its own Commit");
    assert_eq!(committed, Some(0));
    assert_eq!(service.context().state, ConsensusState::Committed);
    assert!(!service.is_running());
}

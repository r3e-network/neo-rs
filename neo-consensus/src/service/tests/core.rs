use super::helpers::{create_test_validators, create_validators_with_keys, sign_payload};
use crate::ConsensusService;
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use crate::{ConsensusError, ConsensusMessageType};
use neo_primitives::UInt256;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_consensus_service_new() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    assert!(!service.is_running());
    assert_eq!(service.context().validator_count(), 7);
}

#[tokio::test]
async fn test_consensus_start() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    assert!(service.is_running());
    assert_eq!(service.context().block_index, 100);
}

#[tokio::test]
async fn test_consensus_not_validator() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, None, vec![], tx);

    let result = service.start(100, 1000, UInt256::zero(), 0);
    assert!(matches!(result, Err(ConsensusError::NotValidator)));
}

#[tokio::test]
async fn test_primary_calculation() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service.start(0, 1000, UInt256::zero(), 0).unwrap();
    assert!(service.context().is_primary());

    service.start(1, 1000, UInt256::zero(), 0).unwrap();
    assert!(!service.context().is_primary());
}

#[tokio::test]
async fn test_message_deduplication() {
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), keys[0].to_vec(), tx);

    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    let msg = PrepareRequestMessage::new(100, 0, 2, 0, UInt256::zero(), 1234, 5678, vec![]);
    let mut payload = ConsensusPayload::new(
        0x4E454F,
        100,
        2,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[2]);
    let msg_hash = service.dbft_payload_hash(&payload).unwrap();

    service.process_message(payload.clone()).unwrap();
    assert!(service.context().has_seen_message(&msg_hash));
    assert!(service.context().prepare_request_received);

    service.process_message(payload).unwrap();

    drop(service);
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    assert_eq!(
        events.len(),
        1,
        "duplicate payload must not emit a second event"
    );
    assert!(matches!(
        events.first(),
        Some(crate::ConsensusEvent::BroadcastMessage(payload))
            if payload.message_type == ConsensusMessageType::PrepareResponse
    ));
}

#[tokio::test]
async fn test_message_cache_cleared_on_new_block() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        1,
        0,
        ConsensusMessageType::PrepareRequest,
        vec![1, 2, 3, 4],
    );

    let _ = service.process_message(payload.clone());

    service.start(101, 2000, UInt256::zero(), 0).unwrap();

    let result = service.process_message(payload);
    assert!(matches!(result, Err(ConsensusError::WrongBlock { .. })));
}

#[tokio::test]
async fn test_replay_attack_prevention() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        1,
        0,
        ConsensusMessageType::ChangeView,
        vec![5, 6, 7, 8],
    );

    let msg_hash = service.dbft_payload_hash(&payload).unwrap();

    assert!(!service.context().has_seen_message(&msg_hash));

    // The payload carries an invalid (empty) witness, so the ChangeView handler
    // rejects it and it is NOT recorded as seen. This is the anti-cache-poison
    // property: a forged-witness payload must not silence a later genuine signed
    // message that hashes to the same value. (Valid-message dedup is covered by
    // `test_message_deduplication`.)
    let result = service.process_message(payload);
    assert!(matches!(
        result,
        Err(ConsensusError::SignatureVerificationFailed { .. })
    ));
    assert!(!service.context().has_seen_message(&msg_hash));
}

#[tokio::test]
async fn invalid_validator_index_rejected() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let msg = PrepareRequestMessage::new(0, 99, 0, 0, UInt256::zero(), 1_000, 1, Vec::new());
    let payload = ConsensusPayload::new(
        network,
        0,
        99,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );

    let result = service.process_message(payload);
    assert!(matches!(
        result,
        Err(ConsensusError::InvalidValidatorIndex(_))
    ));
}

#[tokio::test]
async fn backup_on_transactions_received_is_noop() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, _keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), vec![], tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    service
        .on_transactions_received(vec![UInt256::from([0x11; 32])])
        .unwrap();

    assert!(rx.try_recv().is_err());
    assert!(service.context().proposed_tx_hashes.is_empty());
}

#[tokio::test]
async fn primary_rotation_across_blocks_is_deterministic() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service.start(0, 0, UInt256::zero(), 0).unwrap();
    assert!(service.context().is_primary());

    service.start(1, 0, UInt256::zero(), 0).unwrap();
    assert!(!service.context().is_primary());
    assert_eq!(service.context().primary_index(), 1);

    service.start(2, 0, UInt256::zero(), 0).unwrap();
    assert!(!service.context().is_primary());
    assert_eq!(service.context().primary_index(), 2);
}

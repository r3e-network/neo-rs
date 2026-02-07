use super::helpers::{create_test_validators, create_validators_with_keys};
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use crate::ConsensusService;
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
#[ignore = "TODO: Update test for new security requirements - messages now require valid witness/signatures"]
async fn test_message_deduplication() {
    let (tx, mut rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    let msg = PrepareRequestMessage::new(100, 0, 2, 0, UInt256::zero(), 1234, 5678, vec![]);
    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        2,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );

    let result1 = service.process_message(payload.clone());
    if let Err(ref e) = result1 {
        eprintln!("First message processing failed: {:?}", e);
    }

    let _result2 = service.process_message(payload.clone());

    drop(service);
    let mut event_count = 0;
    while rx.try_recv().is_ok() {
        event_count += 1;
    }
    assert!(event_count >= 1);
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

    let _ = service.process_message(payload.clone());

    assert!(service.context().has_seen_message(&msg_hash));

    let result = service.process_message(payload);

    assert!(result.is_ok());

    assert!(service.context().has_seen_message(&msg_hash));
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

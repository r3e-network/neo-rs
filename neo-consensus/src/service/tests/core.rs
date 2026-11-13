use super::helpers::{create_test_validators, create_validators_with_keys, sign_payload};
use crate::ConsensusService;
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use crate::service::helpers::current_timestamp;
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

    service
        .start(100, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");

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

    service
        .start(0, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(service.context().is_primary());

    service
        .start(1, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(!service.context().is_primary());
}

#[tokio::test]
async fn test_message_deduplication() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(7);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(100, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");

    // Primary for block 100, view 0 is `(100 - 0) % 7 == 2`.
    assert_eq!(service.context().primary_index(), 2);

    let msg = PrepareRequestMessage::new(100, 0, 2, 0, UInt256::zero(), 1234, 5678, vec![]);
    let mut payload = ConsensusPayload::new(
        network,
        100,
        2,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[2]);
    let msg_hash = service.dbft_payload_hash(&payload).expect("payload hash");

    // First delivery is accepted, cached, and answered with our PrepareResponse.
    service
        .process_message(payload.clone())
        .expect("first delivery accepted");
    assert!(service.context().has_seen_message(&msg_hash));
    assert!(service.context().prepare_request_received);
    assert!(!drain_events(&mut rx).is_empty());

    // Re-delivery of the identical payload is dropped before handler dispatch:
    // it is not an error, and it must not emit a second PrepareResponse.
    service
        .process_message(payload)
        .expect("duplicate is ignored, not rejected");
    let replayed = drain_events(&mut rx);
    assert!(
        replayed.is_empty(),
        "duplicate message must not produce new events, got {}",
        replayed.len()
    );

    // Our own PrepareResponse was recorded exactly once.
    assert_eq!(service.context().prepare_responses.len(), 1);
}

fn drain_events(rx: &mut mpsc::Receiver<crate::ConsensusEvent>) -> Vec<crate::ConsensusEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

#[tokio::test]
async fn test_message_cache_cleared_on_new_block() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service
        .start(100, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        1,
        0,
        ConsensusMessageType::PrepareRequest,
        vec![1, 2, 3, 4],
    );

    let _ = service.process_message(payload.clone());

    service
        .start(101, 2000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let result = service.process_message(payload);
    assert!(matches!(result, Err(ConsensusError::WrongBlock { .. })));
}

#[tokio::test]
async fn test_replay_attack_prevention() {
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(0x4E454F, validators, Some(0), vec![], tx);

    service
        .start(100, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");

    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        1,
        0,
        ConsensusMessageType::ChangeView,
        vec![5, 6, 7, 8],
    );

    let msg_hash = service
        .dbft_payload_hash(&payload)
        .expect("should compute payload hash");

    assert!(!service.context().has_seen_message(&msg_hash));

    let _ = service.process_message(payload.clone());

    // Malformed payloads are not marked seen until handler validation succeeds.
    assert!(!service.context().has_seen_message(&msg_hash));

    let result = service.process_message(payload);

    assert!(result.is_err());
    assert!(!service.context().has_seen_message(&msg_hash));
}

#[tokio::test]
async fn replayed_rejected_signature_message_skips_reverification() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(7);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service
        .start(100, 1000, UInt256::zero(), 0)
        .expect("consensus service should start");

    // Primary for block 100, view 0 is validator 2.
    let msg = PrepareRequestMessage::new(100, 0, 2, 0, UInt256::zero(), 1234, 5678, vec![]);
    let mut payload = ConsensusPayload::new(
        network,
        100,
        2,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );
    // Sign with the wrong validator's key: a real 64-byte secp256r1 signature
    // that fails verification (a length error would skip the crypto path and
    // not exercise the amplification guard).
    sign_payload(&service, &mut payload, &keys[3]);
    let msg_hash = service.dbft_payload_hash(&payload).expect("payload hash");

    // First delivery actually executes the secp256r1 verification (non-zero
    // executed count) and fails.
    let result = service.process_message(payload.clone());
    assert!(matches!(
        result,
        Err(ConsensusError::SignatureVerificationFailed { .. })
    ));
    let executed_after_first = service.signature_verification_count.get();
    assert_eq!(
        executed_after_first, 1,
        "first delivery must execute exactly one secp256r1 verification"
    );
    assert!(
        service
            .context()
            .has_recently_rejected_message(&msg_hash, current_timestamp())
    );

    // Replaying the identical rejected payload must be dropped before any
    // secp256r1 work: Ok(()) with zero additional verifications.
    service
        .process_message(payload)
        .expect("rejected replay is dropped, not an error");
    assert_eq!(
        service.signature_verification_count.get(),
        executed_after_first,
        "rejected replay must not trigger another secp256r1 verification"
    );

    // A fresh payload (different data, therefore a different hash) is still
    // verified, proving the cache suppresses only previously rejected hashes.
    let other = PrepareRequestMessage::new(100, 0, 2, 0, UInt256::zero(), 4321, 8765, vec![]);
    let mut fresh_payload = ConsensusPayload::new(
        network,
        100,
        2,
        0,
        ConsensusMessageType::PrepareRequest,
        other.serialize(),
    );
    sign_payload(&service, &mut fresh_payload, &keys[3]);
    assert!(matches!(
        service.process_message(fresh_payload),
        Err(ConsensusError::SignatureVerificationFailed { .. })
    ));
    assert_eq!(
        service.signature_verification_count.get(),
        executed_after_first + 1,
        "fresh payloads must still reach secp256r1 verification"
    );
}

#[tokio::test]
async fn invalid_validator_index_rejected() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

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

    service
        .start(0, 1_000, UInt256::zero(), 0)
        .expect("consensus service should start");

    service
        .on_transactions_received(vec![UInt256::from([0x11; 32])])
        .expect("should process transactions");

    assert!(rx.try_recv().is_err());
    assert!(service.context().proposed_tx_hashes.is_empty());
}

#[tokio::test]
async fn primary_rotation_across_blocks_is_deterministic() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let validators = create_test_validators(7);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service
        .start(0, 0, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(service.context().is_primary());

    service
        .start(1, 0, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(!service.context().is_primary());
    assert_eq!(service.context().primary_index(), 1);

    service
        .start(2, 0, UInt256::zero(), 0)
        .expect("consensus service should start");
    assert!(!service.context().is_primary());
    assert_eq!(service.context().primary_index(), 2);
}

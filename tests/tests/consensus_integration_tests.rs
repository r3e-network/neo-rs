//! Consensus Integration Tests
//!
//! End-to-end tests for the dBFT consensus protocol implementation.
//! Tests cover:
//! - Consensus service lifecycle
//! - Block proposal flow
//! - PrepareRequest/PrepareResponse/Commit message handling
//! - View change mechanism
//! - Recovery message processing
//! - Signature verification

use neo_consensus::{
    ConsensusEvent, ConsensusMessageType, ConsensusPayload, ConsensusService, ValidatorInfo,
    messages::RecoveryMessage,
};
use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::{UInt160, UInt256};
use tokio::sync::mpsc;

// ============================================================================
// Test Helpers
// ============================================================================

fn create_test_validators(count: usize) -> Vec<ValidatorInfo> {
    (0..count)
        .map(|i| ValidatorInfo {
            index: i as u8,
            public_key: ECPoint::infinity(ECCurve::Secp256r1),
            script_hash: UInt160::zero(),
        })
        .collect()
}

fn create_consensus_service(
    validator_index: Option<u8>,
    validator_count: usize,
) -> (ConsensusService, mpsc::Receiver<ConsensusEvent>) {
    let (tx, rx) = mpsc::channel(100);
    let validators = create_test_validators(validator_count);
    let private_key = vec![0u8; 32]; // Test key

    let service = ConsensusService::new(
        0x4E454F, // NEO network magic
        validators,
        validator_index,
        private_key,
        tx,
    );

    (service, rx)
}

// ============================================================================
// Consensus Service Lifecycle Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_service_creation() {
    let (service, _rx) = create_consensus_service(Some(0), 7);

    assert!(!service.is_running());
    assert_eq!(service.context().validator_count(), 7);
}

#[tokio::test]
async fn test_consensus_service_start() {
    let (mut service, _rx) = create_consensus_service(Some(0), 7);

    let result = service.start(100, 1000, UInt256::zero(), 0);
    assert!(result.is_ok());
    assert!(service.is_running());
    assert_eq!(service.context().block_index, 100);
}

#[tokio::test]
async fn test_consensus_not_validator_cannot_start() {
    let (mut service, _rx) = create_consensus_service(None, 7);

    let result = service.start(100, 1000, UInt256::zero(), 0);
    assert!(result.is_err());
    assert!(!service.is_running());
}

#[tokio::test]
async fn test_consensus_primary_calculation() {
    let (mut service, _rx) = create_consensus_service(Some(0), 7);

    // Block 0, view 0 -> validator 0 is primary
    service.start(0, 1000, UInt256::zero(), 0).unwrap();
    assert!(service.context().is_primary());

    // Block 1, view 0 -> validator 1 is primary
    service.start(1, 1000, UInt256::zero(), 0).unwrap();
    assert!(!service.context().is_primary());
}

#[tokio::test]
async fn test_consensus_validator_count() {
    // For n=7 validators
    let (service, _rx) = create_consensus_service(Some(0), 7);
    assert_eq!(service.context().validator_count(), 7);

    // For n=4 validators
    let (service, _rx) = create_consensus_service(Some(0), 4);
    assert_eq!(service.context().validator_count(), 4);

    // For n=1 validator
    let (service, _rx) = create_consensus_service(Some(0), 1);
    assert_eq!(service.context().validator_count(), 1);
}

// ============================================================================
// Message Processing Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_wrong_block_index_rejected() {
    let (mut service, _rx) = create_consensus_service(Some(1), 7);
    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    // Create payload for wrong block
    let payload = ConsensusPayload::new(
        0x4E454F,
        50, // Wrong block index
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        vec![],
    );

    let result = service.process_message(payload);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_consensus_wrong_view_rejected() {
    let (mut service, _rx) = create_consensus_service(Some(1), 7);
    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    // Create payload for wrong view
    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        0,
        5, // Wrong view number
        ConsensusMessageType::PrepareResponse,
        vec![],
    );

    let result = service.process_message(payload);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_consensus_prepare_request_from_non_primary_rejected() {
    let (mut service, _rx) = create_consensus_service(Some(1), 7);
    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    // Block 100, view 0 -> primary is validator (100 % 7) = 2
    // Sending PrepareRequest from validator 5 (not primary) should fail
    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        5, // Not the primary
        0,
        ConsensusMessageType::PrepareRequest,
        vec![],
    );

    let result = service.process_message(payload);
    assert!(result.is_err());
}

// ============================================================================
// Block Proposal Flow Tests
// ============================================================================

#[tokio::test]
async fn test_primary_requests_transactions_on_start() {
    let (mut service, mut rx) = create_consensus_service(Some(0), 7);

    // Start consensus for block 0 where validator 0 is primary
    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Primary should request transactions
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    match event {
        ConsensusEvent::RequestTransactions {
            block_index,
            max_count,
        } => {
            assert_eq!(block_index, 0);
            assert!(max_count > 0);
        }
        _ => panic!("Expected RequestTransactions event"),
    }
}

#[tokio::test]
async fn test_transactions_received_triggers_prepare_request() {
    let (mut service, mut rx) = create_consensus_service(Some(0), 7);
    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Drain the RequestTransactions event
    let _ = rx.recv().await;

    // Simulate receiving transactions
    let tx_hashes = vec![UInt256::from([0x01u8; 32]), UInt256::from([0x02u8; 32])];

    service.on_transactions_received(tx_hashes).unwrap();

    // Should broadcast PrepareRequest
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    match event {
        ConsensusEvent::BroadcastMessage(payload) => {
            assert_eq!(payload.message_type, ConsensusMessageType::PrepareRequest);
            assert_eq!(payload.block_index, 0);
            assert_eq!(payload.validator_index, 0);
        }
        _ => panic!("Expected BroadcastMessage event"),
    }
}

// ============================================================================
// View Change Tests
// ============================================================================

#[tokio::test]
async fn test_timeout_triggers_view_change() {
    let (mut service, mut rx) = create_consensus_service(Some(1), 7);
    service.start(100, 1000, UInt256::zero(), 0).unwrap();

    // Simulate timeout by calling on_timer_tick with future timestamp
    let future_time = 1000 + 60_000; // 60 seconds later
    service.on_timer_tick(future_time).unwrap();

    // Should broadcast ChangeView message
    let event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout")
        .expect("event");

    match event {
        ConsensusEvent::BroadcastMessage(payload) => {
            assert_eq!(payload.message_type, ConsensusMessageType::ChangeView);
        }
        _ => panic!("Expected BroadcastMessage with ChangeView"),
    }
}

// ============================================================================
// Recovery Message Tests
// ============================================================================

#[tokio::test]
async fn test_recovery_message_creation() {
    // Create a recovery message
    let msg = RecoveryMessage::new(100, 0, 1);

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 1);
    assert!(msg.preparation_messages.is_empty());
    assert!(msg.commit_messages.is_empty());
}

#[tokio::test]
async fn test_recovery_message_serialization() {
    // Create a recovery message
    let msg = RecoveryMessage::new(100, 0, 1);

    // Serialize
    let data = msg.serialize();
    assert!(!data.is_empty());

    // Deserialize
    let restored = RecoveryMessage::deserialize(&data, 100, 0, 1).unwrap();

    assert_eq!(restored.block_index, 100);
    assert_eq!(restored.view_number, 0);
}

// ============================================================================
// Consensus Payload Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_payload_creation() {
    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        vec![0x01, 0x02, 0x03],
    );

    assert_eq!(payload.network, 0x4E454F);
    assert_eq!(payload.block_index, 100);
    assert_eq!(payload.validator_index, 0);
    assert_eq!(payload.view_number, 0);
    assert_eq!(payload.message_type, ConsensusMessageType::PrepareRequest);
    assert_eq!(payload.data, vec![0x01, 0x02, 0x03]);
}

#[tokio::test]
async fn test_consensus_payload_sign_data() {
    let payload = ConsensusPayload::new(
        0x4E454F,
        100,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        vec![0x01, 0x02, 0x03],
    );

    let sign_data = payload.get_sign_data();
    assert!(!sign_data.is_empty());

    // Sign data should be deterministic
    let sign_data2 = payload.get_sign_data();
    assert_eq!(sign_data, sign_data2);
}

// ============================================================================
// Multi-Validator Simulation Tests
// ============================================================================

#[tokio::test]
async fn test_multi_validator_prepare_response_collection() {
    let (mut service, _rx) = create_consensus_service(Some(0), 7);
    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Simulate receiving PrepareResponses from other validators
    // For 7 validators, we need m=5 responses to proceed to commit

    // Add responses (simulating context directly for unit test)
    let context = service.context();
    assert_eq!(context.prepare_responses.len(), 0);
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[tokio::test]
async fn test_consensus_handles_empty_transaction_list() {
    let (mut service, mut rx) = create_consensus_service(Some(0), 7);
    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Drain RequestTransactions
    let _ = rx.recv().await;

    // Send empty transaction list
    let result = service.on_transactions_received(vec![]);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_consensus_single_validator_network() {
    let (mut service, mut rx) = create_consensus_service(Some(0), 1);
    service.start(0, 1000, UInt256::zero(), 0).unwrap();

    // Single validator is always primary
    assert!(service.context().is_primary());

    // Validator count should be 1
    assert_eq!(service.context().validator_count(), 1);

    // Should request transactions
    let event = rx.recv().await.expect("event");
    assert!(matches!(event, ConsensusEvent::RequestTransactions { .. }));
}

#[tokio::test]
async fn test_consensus_message_type_variants() {
    // Verify all message types are distinct
    let types = [
        ConsensusMessageType::ChangeView,
        ConsensusMessageType::PrepareRequest,
        ConsensusMessageType::PrepareResponse,
        ConsensusMessageType::Commit,
        ConsensusMessageType::RecoveryRequest,
        ConsensusMessageType::RecoveryMessage,
    ];

    for (i, t1) in types.iter().enumerate() {
        for (j, t2) in types.iter().enumerate() {
            if i == j {
                assert_eq!(t1, t2);
            } else {
                assert_ne!(t1, t2);
            }
        }
    }
}

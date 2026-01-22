use super::helpers::{create_validators_with_keys, sign_commit, sign_payload};
use crate::messages::{
    CommitMessage, ConsensusPayload, PrepareRequestMessage, PrepareResponseMessage,
};
use crate::{ConsensusError, ConsensusMessageType};
use crate::{ConsensusEvent, ConsensusService};
use neo_primitives::UInt256;
use tokio::sync::mpsc;

use super::super::helpers::{
    compute_header_hash, compute_merkle_root, compute_next_consensus_address,
};

#[tokio::test]
async fn consensus_round_emits_block_committed() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

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

    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    for validator_index in 1..=2 {
        let response = PrepareResponseMessage::new(0, 0, validator_index, preparation_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::PrepareResponse,
            response.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    let block_hash = service.context().proposed_block_hash.expect("block hash");

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

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert_eq!(committed, Some(0));
    assert_eq!(prepare_payload.block_index, 0);
}

#[tokio::test]
async fn prepare_request_with_wrong_primary_rejected() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let msg = PrepareRequestMessage::new(0, 0, 1, 0, UInt256::zero(), 1_000, 1, Vec::new());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    let result = service.process_message(payload);
    assert!(matches!(result, Err(ConsensusError::InvalidPrimary { .. })));
}

#[tokio::test]
async fn prepare_response_rejects_mismatched_hash() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

    let wrong_hash = UInt256::from_bytes(&[0x22; 32]).expect("hash");
    let response = PrepareResponseMessage::new(0, 0, 1, wrong_hash);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    let result = service.process_message(payload);
    assert!(matches!(result, Err(ConsensusError::HashMismatch { .. })));
}

#[tokio::test]
async fn prepare_response_with_wrong_view_ignored() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, _keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let response = PrepareResponseMessage::new(0, 1, 1, UInt256::zero());
    let payload = ConsensusPayload::new(
        network,
        0,
        1,
        1,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );

    let result = service.process_message(payload);
    assert!(result.is_ok());
    assert!(service.context().prepare_responses.is_empty());
}

#[tokio::test]
async fn prepare_response_duplicate_from_same_validator_rejected() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    let response = PrepareResponseMessage::new(0, 0, 1, preparation_hash);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    assert!(service.process_message(payload).is_ok());

    let wrong_hash = UInt256::from_bytes(&[0x22; 32]).expect("hash");
    let conflicting_response = PrepareResponseMessage::new(0, 0, 1, wrong_hash);
    let mut conflicting_payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        conflicting_response.serialize(),
    );
    sign_payload(&service, &mut conflicting_payload, &keys[1]);

    let result = service.process_message(conflicting_payload);
    assert!(matches!(result, Err(ConsensusError::AlreadyReceived(1))));
}

#[tokio::test]
async fn byzantine_conflicting_prepare_responses_do_not_replace_first() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    let response = PrepareResponseMessage::new(0, 0, 1, preparation_hash);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);
    assert!(service.process_message(payload).is_ok());

    let wrong_hash = UInt256::from_bytes(&[0x22; 32]).expect("hash");
    let conflicting_response = PrepareResponseMessage::new(0, 0, 1, wrong_hash);
    let mut conflicting_payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        conflicting_response.serialize(),
    );
    sign_payload(&service, &mut conflicting_payload, &keys[1]);

    let result = service.process_message(conflicting_payload);
    assert!(matches!(result, Err(ConsensusError::AlreadyReceived(1))));

    assert_eq!(service.context().prepare_responses.len(), 1);
    assert!(service.context().prepare_responses.contains_key(&1));
}

#[tokio::test]
async fn prepare_response_with_wrong_block_rejected() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(2, 1_000, UInt256::zero(), 0).unwrap();

    let response = PrepareResponseMessage::new(1, 0, 1, UInt256::zero());
    let mut payload = ConsensusPayload::new(
        network,
        1,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    let result = service.process_message(payload);
    assert!(matches!(result, Err(ConsensusError::WrongBlock { .. })));
}

#[tokio::test]
async fn prepare_response_with_future_block_is_ignored() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(2, 1_000, UInt256::zero(), 0).unwrap();

    let response = PrepareResponseMessage::new(3, 0, 1, UInt256::zero());
    let mut payload = ConsensusPayload::new(
        network,
        3,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    assert!(service.process_message(payload).is_ok());
    assert!(service.context().prepare_responses.is_empty());
}

#[tokio::test]
async fn primary_broadcasts_prepare_request_with_transactions() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    let tx_hashes = vec![UInt256::from([0x11; 32]), UInt256::from([0x22; 32])];
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

    let payload = prepare_payload.expect("prepare request payload");
    let msg = PrepareRequestMessage::deserialize_body(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    )
    .expect("prepare request message");

    assert_eq!(msg.transaction_hashes, tx_hashes);
    assert_eq!(service.context().proposed_tx_hashes, tx_hashes);
    assert_eq!(msg.block_index, 0);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 0);

    let expected_hash = service.dbft_payload_hash(&payload).expect("payload hash");
    assert_eq!(service.context().preparation_hash, Some(expected_hash));

    let merkle_root = compute_merkle_root(&msg.transaction_hashes);
    let next_consensus = compute_next_consensus_address(&service.context().validators);
    let expected_block_hash = compute_header_hash(
        msg.version,
        msg.prev_hash,
        merkle_root,
        msg.timestamp,
        msg.nonce,
        msg.block_index,
        service.context().primary_index(),
        next_consensus,
    );
    assert_eq!(
        service.context().proposed_block_hash,
        Some(expected_block_hash)
    );
}

#[tokio::test]
async fn multi_round_prepare_requests_rotate_primary() {
    let network = 0x4E454F;
    let (validators, keys) = create_validators_with_keys(4);
    let (tx0, mut rx0) = mpsc::channel(100);
    let mut service0 =
        ConsensusService::new(network, validators.clone(), Some(0), keys[0].to_vec(), tx0);

    service0.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx0.try_recv().is_ok() {}

    let first_txs = vec![UInt256::from([0x10; 32])];
    service0
        .on_transactions_received(first_txs.clone())
        .unwrap();

    let mut first_prepare = None;
    while let Ok(event) = rx0.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                first_prepare = Some(payload);
                break;
            }
        }
    }

    let first_prepare = first_prepare.expect("first prepare");
    let first_msg = PrepareRequestMessage::deserialize_body(
        &first_prepare.data,
        first_prepare.block_index,
        first_prepare.view_number,
        first_prepare.validator_index,
    )
    .expect("first prepare msg");
    assert_eq!(first_msg.transaction_hashes, first_txs);
    assert_eq!(first_msg.block_index, 0);
    assert_eq!(first_msg.validator_index, 0);

    let (tx1, mut rx1) = mpsc::channel(100);
    let mut service1 = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx1);
    service1.start(1, 2_000, UInt256::zero(), 0).unwrap();
    while rx1.try_recv().is_ok() {}

    let second_txs = vec![UInt256::from([0x20; 32])];
    service1
        .on_transactions_received(second_txs.clone())
        .unwrap();

    let mut second_prepare = None;
    while let Ok(event) = rx1.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                second_prepare = Some(payload);
                break;
            }
        }
    }

    let second_prepare = second_prepare.expect("second prepare");
    let second_msg = PrepareRequestMessage::deserialize_body(
        &second_prepare.data,
        second_prepare.block_index,
        second_prepare.view_number,
        second_prepare.validator_index,
    )
    .expect("second prepare msg");
    assert_eq!(second_msg.transaction_hashes, second_txs);
    assert_eq!(second_msg.block_index, 1);
    assert_eq!(second_msg.validator_index, 1);
}

#[tokio::test]
async fn prepare_responses_trigger_commit_broadcast() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    service.on_transactions_received(Vec::new()).unwrap();

    let prepare_payload = loop {
        match rx.try_recv() {
            Ok(ConsensusEvent::BroadcastMessage(payload))
                if payload.message_type == ConsensusMessageType::PrepareRequest =>
            {
                break payload;
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    };

    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    for validator_index in 1..=2 {
        let response = PrepareResponseMessage::new(0, 0, validator_index, preparation_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
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
    assert_eq!(commit_payload.validator_index, 0);
    assert_eq!(commit_payload.block_index, 0);
    assert_eq!(commit_payload.view_number, 0);
    assert_eq!(commit_payload.data.len(), 64);
    assert_eq!(commit_payload.block_index, prepare_payload.block_index);
}

#[tokio::test]
async fn commits_reach_threshold_emit_block_committed() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

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

    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    for validator_index in 1..=2 {
        let response = PrepareResponseMessage::new(0, 0, validator_index, preparation_hash);
        let mut payload = ConsensusPayload::new(
            network,
            0,
            validator_index,
            0,
            ConsensusMessageType::PrepareResponse,
            response.serialize(),
        );
        sign_payload(&service, &mut payload, &keys[validator_index as usize]);
        service.process_message(payload).unwrap();
    }

    let block_hash = service.context().proposed_block_hash.expect("block hash");

    for validator_index in 1..=2 {
        let signature = sign_commit(network, &block_hash, &keys[validator_index as usize]);
        let commit = CommitMessage::new(0, 0, validator_index, signature.clone());
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

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert_eq!(committed, Some(0));
    assert_eq!(prepare_payload.block_index, 0);
}

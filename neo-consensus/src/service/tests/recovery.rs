use super::helpers::{create_validators_with_keys, sign_commit, sign_payload};
use crate::{ConsensusEvent, ConsensusService};
use crate::messages::{
    ChangeViewPayloadCompact, CommitMessage, CommitPayloadCompact, ConsensusPayload,
    PreparationPayloadCompact, PrepareRequestMessage, PrepareResponseMessage, RecoveryMessage,
    RecoveryRequestMessage,
};
use crate::ConsensusMessageType;
use neo_primitives::UInt256;
use tokio::sync::mpsc;

use super::super::helpers::{
    compute_header_hash, compute_merkle_root, compute_next_consensus_address,
};

#[tokio::test]
async fn recovery_request_broadcasts_recovery_message() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(2), keys[2].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let msg = RecoveryRequestMessage::new(0, 0, 1, 1_234);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut recovery_sent = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::RecoveryMessage {
                recovery_sent = Some(payload);
                break;
            }
        }
    }

    assert!(recovery_sent.is_some());
}

#[tokio::test]
async fn recovery_request_ignored_by_non_selected_validator() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let msg = RecoveryRequestMessage::new(0, 0, 1, 1_234);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut recovery_sent = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::RecoveryMessage {
                recovery_sent = true;
                break;
            }
        }
    }

    assert!(!recovery_sent);
}

#[tokio::test]
async fn recovery_request_responds_when_commit_sent() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service
        .context
        .add_commit(0, vec![0u8; 64])
        .expect("commit");

    let msg = RecoveryRequestMessage::new(0, 0, 1, 1_234);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryRequest,
        msg.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut recovery_sent = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::RecoveryMessage {
                recovery_sent = Some(payload);
                break;
            }
        }
    }

    assert!(recovery_sent.is_some());
}

#[tokio::test]
async fn recovery_message_change_view_triggers_view_change() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let mut recovery = RecoveryMessage::new(0, 1, 1);
    recovery.change_view_messages = vec![
        ChangeViewPayloadCompact {
            validator_index: 1,
            original_view_number: 0,
            timestamp: 1_100,
            invocation_script: Vec::new(),
        },
        ChangeViewPayloadCompact {
            validator_index: 2,
            original_view_number: 0,
            timestamp: 1_200,
            invocation_script: Vec::new(),
        },
        ChangeViewPayloadCompact {
            validator_index: 3,
            original_view_number: 0,
            timestamp: 1_300,
            invocation_script: Vec::new(),
        },
    ];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        1,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut view_changed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::ViewChanged { old_view, new_view, .. } = event {
            view_changed = Some((old_view, new_view));
            break;
        }
    }

    assert_eq!(view_changed, Some((0, 1)));
    assert_eq!(service.context().view_number, 1);
}

#[tokio::test]
async fn recovery_message_ignores_commits_for_wrong_view() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.commit_messages = vec![
        CommitPayloadCompact {
            view_number: 1,
            validator_index: 0,
            signature: vec![0u8; 64],
            invocation_script: Vec::new(),
        },
        CommitPayloadCompact {
            view_number: 1,
            validator_index: 1,
            signature: vec![0u8; 64],
            invocation_script: Vec::new(),
        },
        CommitPayloadCompact {
            view_number: 1,
            validator_index: 2,
            signature: vec![0u8; 64],
            invocation_script: Vec::new(),
        },
    ];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert!(committed.is_none());
    assert_eq!(service.context().commits.len(), 0);
}

#[tokio::test]
async fn recovery_message_ignores_invalid_prepare_request_signature() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let prepare_request = PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 1_000, 7, Vec::new());

    let mut bad_payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare_request.serialize(),
    );
    sign_payload(&service, &mut bad_payload, &keys[1]);

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.prepare_request_message = Some(prepare_request);
    recovery.preparation_messages = vec![PreparationPayloadCompact {
        validator_index: 0,
        invocation_script: bad_payload.witness.clone(),
    }];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert!(!service.context().prepare_request_received);
    assert!(service.context().prepare_responses.is_empty());
    assert!(service.context().proposed_tx_hashes.is_empty());
}

#[tokio::test]
async fn recovery_message_ignores_invalid_prepare_response_signature() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.preparation_hash = Some(UInt256::zero());
    recovery.preparation_messages = vec![PreparationPayloadCompact {
        validator_index: 1,
        invocation_script: vec![0xAB; 64],
    }];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert!(service.context().prepare_responses.is_empty());
}

#[tokio::test]
async fn recovery_message_ignores_invalid_commit_signature() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.commit_messages = vec![
        CommitPayloadCompact {
            view_number: 0,
            validator_index: 0,
            signature: vec![0x42; 64],
            invocation_script: vec![0xAA; 64],
        },
        CommitPayloadCompact {
            view_number: 0,
            validator_index: 1,
            signature: vec![0x42; 64],
            invocation_script: vec![0xAA; 64],
        },
        CommitPayloadCompact {
            view_number: 0,
            validator_index: 2,
            signature: vec![0x42; 64],
            invocation_script: vec![0xAA; 64],
        },
    ];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert!(committed.is_none());
    assert!(service.context().commits.is_empty());
}

#[tokio::test]
async fn recovery_message_ignores_prepare_response_with_mismatched_hash() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.preparation_hash = Some(UInt256::from_bytes(&[0xAA; 32]).expect("hash"));

    let mut prep_payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        PrepareResponseMessage::new(0, 0, 1, UInt256::zero()).serialize(),
    );
    sign_payload(&service, &mut prep_payload, &keys[1]);

    recovery.preparation_messages = vec![PreparationPayloadCompact {
        validator_index: 1,
        invocation_script: prep_payload.witness.clone(),
    }];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert!(service.context().prepare_responses.is_empty());
}

#[tokio::test]
async fn recovery_message_with_commits_triggers_block_commit() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service =
        ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let prepare_request = PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 1_000, 7, Vec::new());

    let mut prepare_payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare_request.serialize(),
    );
    sign_payload(&service, &mut prepare_payload, &keys[0]);

    let merkle_root = compute_merkle_root(&prepare_request.transaction_hashes);
    let next_consensus = compute_next_consensus_address(&service.context().validators);
    let block_hash = compute_header_hash(
        prepare_request.version,
        prepare_request.prev_hash,
        merkle_root,
        prepare_request.timestamp,
        prepare_request.nonce,
        prepare_request.block_index,
        service.context().primary_index(),
        next_consensus,
    );

    let mut recovery = RecoveryMessage::new(0, 0, 1);
    recovery.prepare_request_message = Some(prepare_request);
    recovery.preparation_messages = vec![PreparationPayloadCompact {
        validator_index: 0,
        invocation_script: prepare_payload.witness.clone(),
    }];
    recovery.commit_messages = vec![
        {
            let signature = sign_commit(network, &block_hash, &keys[0]);
            let mut payload = ConsensusPayload::new(
                network,
                0,
                0,
                0,
                ConsensusMessageType::Commit,
                CommitMessage::new(0, 0, 0, signature.clone()).serialize(),
            );
            sign_payload(&service, &mut payload, &keys[0]);
            CommitPayloadCompact {
                view_number: 0,
                validator_index: 0,
                signature,
                invocation_script: payload.witness.clone(),
            }
        },
        {
            let signature = sign_commit(network, &block_hash, &keys[1]);
            let mut payload = ConsensusPayload::new(
                network,
                0,
                1,
                0,
                ConsensusMessageType::Commit,
                CommitMessage::new(0, 0, 1, signature.clone()).serialize(),
            );
            sign_payload(&service, &mut payload, &keys[1]);
            CommitPayloadCompact {
                view_number: 0,
                validator_index: 1,
                signature,
                invocation_script: payload.witness.clone(),
            }
        },
        {
            let signature = sign_commit(network, &block_hash, &keys[2]);
            let mut payload = ConsensusPayload::new(
                network,
                0,
                2,
                0,
                ConsensusMessageType::Commit,
                CommitMessage::new(0, 0, 2, signature.clone()).serialize(),
            );
            sign_payload(&service, &mut payload, &keys[2]);
            CommitPayloadCompact {
                view_number: 0,
                validator_index: 2,
                signature,
                invocation_script: payload.witness.clone(),
            }
        },
    ];

    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::RecoveryMessage,
        recovery.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    let mut committed = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted { block_index, .. } = event {
            committed = Some(block_index);
            break;
        }
    }

    assert_eq!(committed, Some(0));
}

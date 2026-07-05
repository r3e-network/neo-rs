use super::*;

#[tokio::test]
async fn backup_prepare_request_with_transactions_requests_exact_hashes() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let tx_hashes = vec![UInt256::from([0x31; 32]), UInt256::from([0x32; 32])];
    let prepare =
        PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 1_001, 99, tx_hashes.clone());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    service.process_message(payload).await.unwrap();

    let event = rx.try_recv().expect("proposal transaction request");
    assert!(matches!(
        event,
        ConsensusEvent::RequestProposalTransactions {
            block_index: 0,
            transaction_hashes
        } if transaction_hashes == tx_hashes
    ));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn backup_rejects_prepare_request_above_protocol_transaction_limit() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_max_transactions_per_block(1);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let tx_hashes = vec![UInt256::from([0x31; 32]), UInt256::from([0x32; 32])];
    let prepare = PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 1_001, 99, tx_hashes);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    let result = service.process_message(payload).await;
    assert!(matches!(
        result,
        Err(ConsensusError::InvalidProposal { message }) if message.contains("MaxTransactionsPerBlock")
    ));
    assert!(!service.context().prepare_request_received);
}

#[tokio::test]
async fn backup_rejects_prepare_request_not_after_previous_timestamp() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);

    service
        .start_with_previous_timestamp(0, 1_000, UInt256::zero(), 5_000, 0)
        .unwrap();

    let prepare = PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 5_000, 99, Vec::new());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    let result = service.process_message(payload).await;
    assert!(matches!(
        result,
        Err(ConsensusError::InvalidProposal { message }) if message.contains("timestamp")
    ));
    assert!(!service.context().prepare_request_received);
}

#[tokio::test]
async fn backup_rejects_prepare_request_too_far_in_future() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);

    service
        .start_with_previous_timestamp(0, current_timestamp(), UInt256::zero(), 0, 0)
        .unwrap();

    let future_timestamp = current_timestamp().saturating_add(9_000);
    let prepare = PrepareRequestMessage::new(
        0,
        0,
        0,
        0,
        UInt256::zero(),
        future_timestamp,
        99,
        Vec::new(),
    );
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    let result = service.process_message(payload).await;
    assert!(matches!(
        result,
        Err(ConsensusError::InvalidProposal { message }) if message.contains("timestamp")
    ));
    assert!(!service.context().prepare_request_received);
}

#[tokio::test]
async fn backup_rejects_prepare_request_with_wrong_prev_hash() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);

    let expected_prev_hash = UInt256::from([0xAA; 32]);
    let wrong_prev_hash = UInt256::from([0xBB; 32]);
    service
        .start_with_previous_timestamp(0, 1_000, expected_prev_hash, 0, 0)
        .unwrap();

    let prepare = PrepareRequestMessage::new(0, 0, 0, 0, wrong_prev_hash, 1_001, 99, Vec::new());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[0]);

    let result = service.process_message(payload).await;
    assert!(matches!(
        result,
        Err(ConsensusError::InvalidProposal { message }) if message.contains("prev_hash")
    ));
    assert_eq!(service.context().prev_hash, expected_prev_hash);
    assert!(!service.context().prepare_request_received);
}

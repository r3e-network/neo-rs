use super::*;

#[tokio::test]
async fn primary_requests_configured_transaction_limit() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);
    service.set_max_transactions_per_block(2);
    service.set_expected_block_time(1_000);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    let deadline = service
        .context()
        .view_start_time
        .saturating_add(service.context().prepare_request_delay());
    service.on_timer_tick(deadline).unwrap();

    let event = rx.try_recv().expect("transaction request");
    assert!(matches!(
        event,
        ConsensusEvent::RequestTransactions {
            block_index: 0,
            max_count: 2,
            ..
        }
    ));
}

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
async fn primary_prepare_request_timestamp_is_after_previous_header() {
    let network = 0x4E454F;
    let (validators, keys) = create_validators_with_keys(4);
    let (tx, mut rx) = mpsc::channel(100);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    let previous_timestamp = current_timestamp() + 60_000;
    service
        .start_with_previous_timestamp(0, 1_000, UInt256::zero(), previous_timestamp, 0)
        .unwrap();
    while rx.try_recv().is_ok() {}

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
    let payload = prepare_payload.expect("prepare request payload");
    let msg = PrepareRequestMessage::deserialize_body(
        &payload.data,
        payload.block_index,
        payload.view_number,
        payload.validator_index,
    )
    .expect("prepare request message");

    assert_eq!(
        msg.timestamp,
        previous_timestamp + 1,
        "C# v3.10.0 clamps PrepareRequest timestamp to max(now, PrevHeader.Timestamp + 1)"
    );
    assert_eq!(service.context().proposed_timestamp, msg.timestamp);
}

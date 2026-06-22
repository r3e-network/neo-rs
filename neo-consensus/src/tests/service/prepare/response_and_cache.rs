use super::*;

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
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), vec![], tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let response = PrepareResponseMessage::new(0, 1, 1, UInt256::zero());
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        1,
        ConsensusMessageType::PrepareResponse,
        response.serialize(),
    );
    sign_payload(&service, &mut payload, &keys[1]);

    let result = service.process_message(payload);
    assert!(result.is_ok());
    assert!(service.context().prepare_responses.is_empty());
}

#[tokio::test]
async fn other_view_commit_without_witness_is_rejected_and_not_cached() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let commit = CommitMessage::new(0, 1, 1, vec![0x11; 64]);
    let payload = ConsensusPayload::new(
        network,
        0,
        1,
        1,
        ConsensusMessageType::Commit,
        commit.serialize(),
    );

    let result = service.process_message(payload);
    assert!(matches!(
        result,
        Err(ConsensusError::SignatureVerificationFailed { .. })
    ));
    assert!(!service.context().commits.contains_key(&1));
}

#[tokio::test]
async fn current_view_commit_before_prepare_request_is_cached_and_revalidated() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(2), keys[2].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let tx_hashes = Vec::new();
    let timestamp = 1_001;
    let nonce = 99;
    let block_hash = proposed_block_hash(&service, &tx_hashes, timestamp, nonce);
    let signature = sign_commit(network, &block_hash, &keys[1]);
    let commit = CommitMessage::new(0, 0, 1, signature.clone());
    let mut commit_payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::Commit,
        commit.serialize(),
    );
    sign_payload(&service, &mut commit_payload, &keys[1]);

    assert!(service.process_message(commit_payload).is_ok());
    assert_eq!(service.context().proposed_block_hash, None);
    assert_eq!(service.context().commits.get(&1), Some(&signature));
    assert!(service.context().commit_invocations.contains_key(&1));

    let prepare =
        PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), timestamp, nonce, tx_hashes);
    let mut prepare_payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut prepare_payload, &keys[0]);

    service.process_message(prepare_payload).unwrap();

    assert_eq!(service.context().proposed_block_hash, Some(block_hash));
    assert_eq!(service.context().commits.get(&1), Some(&signature));
    assert!(service.context().commit_invocations.contains_key(&1));
}

#[tokio::test]
async fn commit_with_trailing_body_bytes_uses_first_64_signature_bytes() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).unwrap();

    let block_hash = service.context().proposed_block_hash.expect("block hash");
    let signature = sign_commit(network, &block_hash, &keys[1]);
    let mut body = signature.clone();
    body.extend_from_slice(&[0xAA, 0xBB]);
    let mut payload = ConsensusPayload::new(network, 0, 1, 0, ConsensusMessageType::Commit, body);
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert_eq!(service.context().commits.get(&1), Some(&signature));
}

#[tokio::test]
async fn invalid_cached_current_view_commit_is_removed_after_prepare_request() {
    let network = 0x4E454F;
    let (tx, _rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(2), keys[2].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let tx_hashes = Vec::new();
    let timestamp = 1_001;
    let nonce = 99;
    let wrong_block_hash = UInt256::from([0xEE; 32]);
    let invalid_signature = sign_commit(network, &wrong_block_hash, &keys[1]);
    let commit = CommitMessage::new(0, 0, 1, invalid_signature.clone());
    let mut commit_payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::Commit,
        commit.serialize(),
    );
    sign_payload(&service, &mut commit_payload, &keys[1]);

    assert!(service.process_message(commit_payload).is_ok());
    assert_eq!(service.context().commits.get(&1), Some(&invalid_signature));
    assert!(service.context().commit_invocations.contains_key(&1));

    let prepare =
        PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), timestamp, nonce, tx_hashes);
    let mut prepare_payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(&service, &mut prepare_payload, &keys[0]);

    service.process_message(prepare_payload).unwrap();

    assert!(!service.context().commits.contains_key(&1));
    assert!(!service.context().commit_view_numbers.contains_key(&1));
    assert!(!service.context().commit_invocations.contains_key(&1));
}

#[tokio::test]
async fn prepare_response_with_trailing_body_bytes_uses_first_hash() {
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
    let mut body = preparation_hash.as_bytes().to_vec();
    body.push(0xCC);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        1,
        0,
        ConsensusMessageType::PrepareResponse,
        body,
    );
    sign_payload(&service, &mut payload, &keys[1]);

    service.process_message(payload).unwrap();

    assert_eq!(
        service.context().prepare_response_hashes.get(&1),
        Some(&preparation_hash)
    );
}

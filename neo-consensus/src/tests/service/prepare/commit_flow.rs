use super::*;

#[tokio::test]
async fn prepare_responses_trigger_commit_broadcast() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    while rx.try_recv().is_ok() {}

    service.on_transactions_received(Vec::new()).await.unwrap();

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
        service.process_message(payload).await.unwrap();
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
    service.on_transactions_received(Vec::new()).await.unwrap();

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
        service.process_message(payload).await.unwrap();
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
        service.process_message(payload).await.unwrap();
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

/// End-to-end: a 4-validator round commits a block, and `BlockData::assemble_block`
/// reconstructs *exactly* the block the validators agreed on — its hash equals the
/// proposed block hash the commit signatures were taken over, and its witness is the
/// M-of-N multi-sig over the validators. This connects the verified consensus state
/// machine to the verified block-assembly output.
#[tokio::test]
async fn committed_round_assembles_into_the_agreed_block() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);

    service.start(0, 1_000, UInt256::zero(), 0).unwrap();
    service.on_transactions_received(Vec::new()).await.unwrap();

    // Drain until the primary's PrepareRequest has been emitted.
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareRequest {
                break;
            }
        }
    }
    let preparation_hash = service
        .context()
        .preparation_hash
        .expect("preparation hash");

    // PrepareResponses from validators 1..=2 (with the primary's prepare → M=3).
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
        service.process_message(payload).await.unwrap();
    }

    let block_hash = service
        .context()
        .proposed_block_hash
        .expect("proposed block hash");

    // Commits from validators 1..=2 (with the primary's own commit → M=3 → committed).
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
        service.process_message(payload).await.unwrap();
    }

    let mut block_data = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BlockCommitted {
            block_data: data, ..
        } = event
        {
            block_data = Some(data);
            break;
        }
    }
    let block_data = block_data.expect("round must commit a block");

    // N = 4 → M = N - (N-1)/3 = 3.
    assert_eq!(block_data.required_signatures, 3);

    // Assemble the final block and prove it IS the block the validators committed to:
    // its hash equals the proposed block hash the commit signatures were taken over.
    let block = block_data
        .assemble_block(0, UInt256::zero(), Vec::new())
        .expect("assemble committed block");
    assert_eq!(
        block.header.hash(),
        block_hash,
        "assembled block must equal the agreed (signed) block"
    );

    // The witness verification script is the M-of-N multi-sig over the validators.
    assert_eq!(
        block.header.witness.verification_script,
        crate::service::helpers::ConsensusBlockFields::multisig_verification_script(
            &block_data.validator_pubkeys
        ),
    );
    assert!(!block.header.witness.invocation_script.is_empty());
}

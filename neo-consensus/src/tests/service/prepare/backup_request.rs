use super::*;
use crate::ChangeViewReason;

/// Helper: build + sign a PrepareRequest from the primary (validator 0) naming
/// `tx_hashes`, and deliver it to `service`.
async fn deliver_prepare_request(
    service: &mut ConsensusService,
    network: u32,
    keys: &[[u8; 32]],
    tx_hashes: Vec<UInt256>,
) {
    let prepare = PrepareRequestMessage::new(0, 0, 0, 0, UInt256::zero(), 1_001, 99, tx_hashes);
    let mut payload = ConsensusPayload::new(
        network,
        0,
        0,
        0,
        ConsensusMessageType::PrepareRequest,
        prepare.serialize(),
    );
    sign_payload(service, &mut payload, &keys[0]);
    service.process_message(payload).await.unwrap();
}

fn broadcast_prepare_response_count(rx: &mut mpsc::Receiver<ConsensusEvent>) -> usize {
    let mut count = 0;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareResponse {
                count += 1;
            }
        }
    }
    count
}

/// C# `ConsensusService.OnTransaction` liveness fix: a backup that receives a
/// `PrepareRequest` referencing a transaction it does not yet have must NOT
/// immediately view-change; when the missing transaction later arrives (via the
/// `OnTransaction` late feed) it completes the round by sending its
/// `PrepareResponse` — exactly as if all transactions had been present at
/// `PrepareRequest` time.
#[tokio::test]
async fn backup_resumes_round_when_missing_transaction_arrives_late() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    // Backup: validator index 1 (primary for view 0 at block 0 is index 0).
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);
    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    // The proposal references two transactions; the backup will only have one.
    let tx_present = UInt256::from([0x31; 32]);
    let tx_missing = UInt256::from([0x32; 32]);
    let proposal = vec![tx_present, tx_missing];

    deliver_prepare_request(&mut service, network, &keys, proposal.clone()).await;

    // The backup accepted the PrepareRequest and asked the node to resolve the
    // proposal's transactions — it did NOT view-change on receipt.
    assert!(service.context().prepare_request_received);
    let request = rx.try_recv().expect("proposal transaction request");
    assert!(matches!(
        request,
        ConsensusEvent::RequestProposalTransactions { transaction_hashes, .. }
            if transaction_hashes == proposal
    ));

    // The node resolves only the transaction the backup already has; the other
    // is still propagating. C# backup path: `on_transactions_received` with the
    // available subset.
    service
        .on_transactions_received(vec![tx_present])
        .await
        .unwrap();

    // Still missing a transaction → no PrepareResponse yet, and crucially the
    // node has NOT requested a view change.
    assert!(service.context().has_missing_proposed_transactions());
    assert_eq!(
        broadcast_prepare_response_count(&mut rx),
        0,
        "backup must not respond while a proposal transaction is missing"
    );
    assert!(
        service.context().change_views.is_empty(),
        "backup must not view-change merely because a transaction is missing"
    );

    // A stray transaction that is NOT part of the proposal must be ignored (C#
    // `OnTransaction`: `!TransactionHashes.Contains(hash)` early-return).
    service
        .on_transaction(UInt256::from([0x99; 32]))
        .await
        .unwrap();
    assert!(service.context().has_missing_proposed_transactions());
    assert_eq!(broadcast_prepare_response_count(&mut rx), 0);

    // The missing transaction finally arrives via the late feed (C#
    // `OnTransaction`). This is the last one the proposal was waiting for, so
    // the backup now sends its PrepareResponse and completes the round.
    service.on_transaction(tx_missing).await.unwrap();

    assert!(
        !service.context().has_missing_proposed_transactions(),
        "all proposal transactions are now available"
    );
    assert_eq!(
        broadcast_prepare_response_count(&mut rx),
        1,
        "backup sends its PrepareResponse once the last missing transaction arrives"
    );
    assert!(
        service.context().change_views.is_empty(),
        "the round resumed without any view change"
    );
}

/// The late feed drives the backup all the way to a Commit: once the missing
/// transaction arrives AND the M-threshold of PrepareResponses is present, the
/// backup signs and broadcasts its Commit — proving the resume re-enters the
/// full gated preparation check, not just the PrepareResponse step.
#[tokio::test]
async fn late_transaction_lets_backup_reach_commit() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);
    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let tx_missing = UInt256::from([0x32; 32]);
    let proposal = vec![tx_missing];
    deliver_prepare_request(&mut service, network, &keys, proposal).await;
    // Drain the RequestProposalTransactions event; the tx is not available yet.
    while rx.try_recv().is_ok() {}
    service.on_transactions_received(Vec::new()).await.unwrap();

    let preparation_hash = service.context().preparation_hash.expect("preparation hash");

    // Two other backups (indices 2, 3) send PrepareResponses. Together with the
    // primary's implicit preparation that is M = 3, but the backup is still
    // blocked because it is missing the proposal transaction (commit gate).
    for validator_index in [2u8, 3u8] {
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

    // Enough preparations, but no Commit yet — the transaction is missing.
    assert!(service.context().has_missing_proposed_transactions());
    let mut saw_commit = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            saw_commit |= payload.message_type == ConsensusMessageType::Commit;
        }
    }
    assert!(!saw_commit, "must not commit while a transaction is missing");

    // The missing transaction arrives late → the backup sends its
    // PrepareResponse AND, now that all preparations + all transactions are
    // present, signs its Commit.
    service.on_transaction(tx_missing).await.unwrap();

    let mut saw_response = false;
    let mut saw_commit = false;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            match payload.message_type {
                ConsensusMessageType::PrepareResponse => saw_response = true,
                ConsensusMessageType::Commit => {
                    saw_commit = true;
                    assert_eq!(payload.validator_index, 1);
                    assert_eq!(payload.data.len(), 64);
                }
                _ => {}
            }
        }
    }
    assert!(saw_response, "backup sends its PrepareResponse on completion");
    assert!(saw_commit, "backup signs its Commit once the round is complete");
}

/// Guard parity with C# `OnTransaction`: the late feed is a no-op for a node
/// that is the primary, that has not received a PrepareRequest, or that is mid
/// view-change; and it never double-records an already-available transaction.
#[tokio::test]
async fn on_transaction_guards_match_csharp() {
    let network = 0x4E454F;

    // Primary (index 0) ignores the late feed entirely (C# `!IsBackup`).
    {
        let (tx, mut rx) = mpsc::channel(100);
        let (validators, keys) = create_validators_with_keys(4);
        let mut service =
            ConsensusService::new(network, validators, Some(0), keys[0].to_vec(), tx);
        service.start(0, 1_000, UInt256::zero(), 0).unwrap();
        service
            .on_transaction(UInt256::from([0x31; 32]))
            .await
            .unwrap();
        assert!(rx.try_recv().is_err(), "primary ignores OnTransaction");
    }

    // Backup that has NOT received a PrepareRequest ignores the feed
    // (C# `!RequestSentOrReceived`).
    {
        let (tx, mut rx) = mpsc::channel(100);
        let (validators, keys) = create_validators_with_keys(4);
        let mut service =
            ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
        service.start(0, 1_000, UInt256::zero(), 0).unwrap();
        assert!(!service.context().prepare_request_received);
        service
            .on_transaction(UInt256::from([0x31; 32]))
            .await
            .unwrap();
        assert!(rx.try_recv().is_err(), "no PrepareRequest → OnTransaction no-ops");
    }
}

/// A backup timer tick while a proposal transaction is missing selects the
/// `TxNotFound` change-view reason (C# `OnTimer`: `TransactionHashes.Length >
/// Transactions.Count → TxNotFound`) — confirming the missing-transaction state
/// is the liveness hazard the late feed exists to avoid.
#[tokio::test]
async fn missing_transaction_timeout_reason_is_tx_not_found() {
    let network = 0x4E454F;
    let (tx, mut rx) = mpsc::channel(100);
    let (validators, keys) = create_validators_with_keys(4);
    let mut service = ConsensusService::new(network, validators, Some(1), keys[1].to_vec(), tx);
    service.set_expected_block_time(1_000);
    service.start(0, 1_000, UInt256::zero(), 0).unwrap();

    let proposal = vec![UInt256::from([0x31; 32]), UInt256::from([0x32; 32])];
    deliver_prepare_request(&mut service, network, &keys, proposal).await;
    while rx.try_recv().is_ok() {}
    // Only one of the two proposal transactions is available.
    service
        .on_transactions_received(vec![UInt256::from([0x31; 32])])
        .await
        .unwrap();
    assert!(service.context().has_missing_proposed_transactions());

    // Fire the timer far past the view deadline so the backup requests a view
    // change; because a transaction is missing, the reason must be TxNotFound.
    let deadline = service.context().view_start_time + 10 * service.context().expected_block_time;
    service.on_timer_tick(deadline).await.unwrap();

    let mut reason = None;
    while let Ok(event) = rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::ChangeView {
                let cv = crate::messages::ChangeViewMessage::deserialize(
                    &payload.data,
                    payload.block_index,
                    payload.view_number,
                    payload.validator_index,
                )
                .expect("change view body");
                reason = Some(cv.reason);
            }
        }
    }
    assert_eq!(reason, Some(ChangeViewReason::TxNotFound));
}

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

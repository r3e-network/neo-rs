use super::*;

#[test]
fn proposal_resolution_caches_unverified_transactions_like_csharp_prepare_request() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let tx = signed_zero_fee_tx(&settings, 0x32);
    let hash = tx.hash();
    assert_eq!(pool.try_add(tx.clone(), &snapshot), VerifyResult::Succeed);
    pool.update_pool_for_block_persisted(&[]);
    assert!(pool.get_verified(&hash).is_none());
    assert!(pool.get(&hash).is_some());

    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
    let (validators, _) = consensus_test_validators(4);
    let available = cache_available_proposal_transactions(
        &[hash],
        &mut cache,
        &pool,
        &snapshot,
        &settings,
        &validators,
    );
    assert_eq!(available.available, vec![hash]);
    assert_eq!(available.rejection_reason, None);
    assert_eq!(cache.get(&hash).map(|tx| tx.hash()), Some(hash));
}

#[test]
fn proposal_resolution_reverifies_unverified_transactions_against_context() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let private = [0x51u8; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    let account = UInt160::from_script(&verification);
    seed_gas_balance(&snapshot, &account, 2_000);

    let first = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5100_0001,
        0,
        1_000,
        Vec::new(),
    );
    let second = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5100_0002,
        0,
        1_000,
        Vec::new(),
    );
    let first_hash = first.hash();
    let second_hash = second.hash();
    assert_eq!(
        pool.try_add(first.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(second.clone(), &snapshot),
        VerifyResult::Succeed
    );
    pool.update_pool_for_block_persisted(&[]);
    assert!(pool.get_verified(&first_hash).is_none());
    assert!(pool.get_verified(&second_hash).is_none());

    seed_gas_balance(&snapshot, &account, 1_500);
    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
    let (validators, _) = consensus_test_validators(4);
    let available = cache_available_proposal_transactions(
        &[first_hash, second_hash],
        &mut cache,
        &pool,
        &snapshot,
        &settings,
        &validators,
    );

    assert_eq!(
        available.available,
        vec![first_hash],
        "C# AddTransaction(tx, true) re-verifies unverified proposal txs against context sender fees"
    );
    assert_eq!(
        available.rejection_reason,
        Some(ChangeViewReason::TxInvalid)
    );
    assert!(cache.contains_key(&first_hash));
    assert!(!cache.contains_key(&second_hash));
}

#[test]
fn proposal_resolution_rejects_unverified_conflicts_against_proposal_hashes() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let target = signed_zero_fee_tx(&settings, 0x52);
    let target_hash = target.hash();
    let (private, public, account) = signing_account(0x53);
    let conflict = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5300_0001,
        0,
        0,
        vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(target_hash),
        )],
    );
    let conflict_hash = conflict.hash();

    assert_eq!(pool.try_add(conflict, &snapshot), VerifyResult::Succeed);
    pool.update_pool_for_block_persisted(&[]);
    assert!(pool.get_verified(&conflict_hash).is_none());
    assert_eq!(pool.try_add(target, &snapshot), VerifyResult::Succeed);
    assert!(pool.get_verified(&target_hash).is_some());

    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
    let (validators, _) = consensus_test_validators(4);
    let available = cache_available_proposal_transactions(
        &[target_hash, conflict_hash],
        &mut cache,
        &pool,
        &snapshot,
        &settings,
        &validators,
    );

    assert_eq!(available.available, vec![target_hash]);
    assert_eq!(
        available.rejection_reason,
        Some(ChangeViewReason::TxInvalid)
    );
    assert!(cache.contains_key(&target_hash));
    assert!(!cache.contains_key(&conflict_hash));
}

#[test]
fn proposal_resolution_rejects_unverified_when_context_conflicts_with_it() {
    neo_native_contracts::install();
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let target = signed_zero_fee_tx(&settings, 0x54);
    let target_hash = target.hash();
    assert_eq!(pool.try_add(target, &snapshot), VerifyResult::Succeed);
    pool.update_pool_for_block_persisted(&[]);
    assert!(pool.get_verified(&target_hash).is_none());

    let (private, public, account) = signing_account(0x55);
    let conflict = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5500_0001,
        0,
        0,
        vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(target_hash),
        )],
    );
    let conflict_hash = conflict.hash();
    assert_eq!(pool.try_add(conflict, &snapshot), VerifyResult::Succeed);
    assert!(pool.get_verified(&conflict_hash).is_some());

    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
    let (validators, _) = consensus_test_validators(4);
    let available = cache_available_proposal_transactions(
        &[conflict_hash, target_hash],
        &mut cache,
        &pool,
        &snapshot,
        &settings,
        &validators,
    );

    assert_eq!(available.available, vec![conflict_hash]);
    assert_eq!(
        available.rejection_reason,
        Some(ChangeViewReason::TxInvalid)
    );
    assert!(cache.contains_key(&conflict_hash));
    assert!(!cache.contains_key(&target_hash));
}

#[test]
fn primary_proposal_selection_stops_before_dbft_max_block_system_fee() {
    let settings = ProtocolSettings::default();
    let (validators, _) = consensus_test_validators(4);
    let (private, public, account) = signing_account(0x61);
    let first = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x6100_0001,
        DBFT_MAX_BLOCK_SYSTEM_FEE,
        0,
        Vec::new(),
    );
    let second = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x6100_0002,
        1,
        0,
        Vec::new(),
    );
    let first_hash = first.hash();
    let second_hash = second.hash();
    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();

    let hashes = select_primary_proposal_transactions(
        vec![PoolItem::new(first), PoolItem::new(second)],
        2,
        &mut cache,
        &validators,
        &settings,
    );

    assert_eq!(hashes, vec![first_hash]);
    assert!(cache.contains_key(&first_hash));
    assert!(
        !cache.contains_key(&second_hash),
        "C# EnsureMaxBlockLimitation breaks before adding the tx that would exceed MaxBlockSystemFee"
    );
}

#[test]
fn proposal_resolution_rejects_full_block_over_dbft_max_block_size() {
    neo_native_contracts::install();
    let mut settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = MemoryPool::new(&settings);
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);
    let (validators, _) = consensus_test_validators(4);
    let tx = signed_zero_fee_tx(&settings, 0x62);
    let hash = tx.hash();
    let oversized_limit = expected_dbft_block_size_without_transactions(1, &validators)
        + <Transaction as Serializable>::size(&tx)
        - 1;
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    settings.max_block_size = oversized_limit as u32;

    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();
    let available = cache_available_proposal_transactions(
        &[hash],
        &mut cache,
        &pool,
        &snapshot,
        &settings,
        &validators,
    );

    assert!(available.available.is_empty());
    assert_eq!(
        available.rejection_reason,
        Some(ChangeViewReason::BlockRejectedByPolicy)
    );
}

#[test]
fn proposal_rejection_reason_matches_csharp_add_transaction_mapping() {
    assert_eq!(
        proposal_rejection_reason(VerifyResult::PolicyFail),
        ChangeViewReason::TxRejectedByPolicy
    );
    assert_eq!(
        proposal_rejection_reason(VerifyResult::HasConflicts),
        ChangeViewReason::TxInvalid
    );
    assert_eq!(
        proposal_rejection_reason(VerifyResult::InsufficientFunds),
        ChangeViewReason::TxInvalid
    );
}

#[tokio::test]
async fn proposal_resolution_requests_change_view_for_invalid_unverified_transaction() {
    neo_native_contracts::install();
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(MemoryPool::new(&settings));
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let private = [0x56u8; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    let account = UInt160::from_script(&verification);
    seed_gas_balance(&snapshot, &account, 2_000);

    let first = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5600_0001,
        0,
        1_000,
        Vec::new(),
    );
    let second = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x5600_0002,
        0,
        1_000,
        Vec::new(),
    );
    let first_hash = first.hash();
    let second_hash = second.hash();
    assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
    assert_eq!(pool.try_add(second, &snapshot), VerifyResult::Succeed);
    pool.update_pool_for_block_persisted(&[]);
    seed_gas_balance(&snapshot, &account, 1_500);

    let (validators, consensus_keys) = consensus_test_validators(4);
    let (event_tx, event_rx) = mpsc::channel(16);
    let mut context = ConsensusContext::new(0, validators.clone(), Some(1), Some(1_000));
    context.prepare_request_received = true;
    context.proposed_tx_hashes = vec![first_hash, second_hash];
    let mut service = ConsensusService::with_context(
        settings.network,
        context,
        consensus_keys[1].to_vec(),
        event_tx,
    );
    service
        .resume_with_next_consensus(10_000, UInt256::zero(), UInt160::zero(), 0)
        .expect("resume backup context");

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        blockchain,
        mempool: pool,
        network,
        settings,
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[1].public_key.clone(),
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    driver
        .on_consensus_event(
            ConsensusEvent::RequestProposalTransactions {
                block_index: 0,
                transaction_hashes: vec![first_hash, second_hash],
            },
            &snapshot,
        )
        .await;

    let mut reason = None;
    while let Ok(event) = driver.event_rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::ChangeView {
                let msg = ChangeViewMessage::deserialize(
                    &payload.data,
                    payload.block_index,
                    payload.view_number,
                    payload.validator_index,
                )
                .expect("change view deserialize");
                reason = Some(msg.reason);
                break;
            }
        }
    }

    assert_eq!(
        reason,
        Some(ChangeViewReason::TxInvalid),
        "C# AddTransaction(tx, true) requests TxInvalid when proposal-local re-verification fails"
    );
}

#[tokio::test]
async fn proposal_resolution_requests_block_rejected_without_prepare_response_for_over_fee_block() {
    neo_native_contracts::install();
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(MemoryPool::new(&settings));
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let private = [0x63u8; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    let account = UInt160::from_script(&verification);
    seed_gas_balance(&snapshot, &account, DBFT_MAX_BLOCK_SYSTEM_FEE + 100);

    let first = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x6300_0001,
        DBFT_MAX_BLOCK_SYSTEM_FEE,
        0,
        Vec::new(),
    );
    let second = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x6300_0002,
        1,
        0,
        Vec::new(),
    );
    let first_hash = first.hash();
    let second_hash = second.hash();
    assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
    assert_eq!(pool.try_add(second, &snapshot), VerifyResult::Succeed);

    let (validators, consensus_keys) = consensus_test_validators(4);
    let (event_tx, event_rx) = mpsc::channel(16);
    let mut context = ConsensusContext::new(0, validators.clone(), Some(1), Some(1_000));
    context.prepare_request_received = true;
    context.proposed_tx_hashes = vec![first_hash, second_hash];
    let mut service = ConsensusService::with_context(
        settings.network,
        context,
        consensus_keys[1].to_vec(),
        event_tx,
    );
    service
        .resume_with_next_consensus(10_000, UInt256::zero(), UInt160::zero(), 0)
        .expect("resume backup context");

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        blockchain,
        mempool: pool,
        network,
        settings,
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[1].public_key.clone(),
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    driver
        .on_consensus_event(
            ConsensusEvent::RequestProposalTransactions {
                block_index: 0,
                transaction_hashes: vec![first_hash, second_hash],
            },
            &snapshot,
        )
        .await;

    let mut reason = None;
    let mut sent_prepare_response = false;
    while let Ok(event) = driver.event_rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            match payload.message_type {
                ConsensusMessageType::ChangeView => {
                    let msg = ChangeViewMessage::deserialize(
                        &payload.data,
                        payload.block_index,
                        payload.view_number,
                        payload.validator_index,
                    )
                    .expect("change view deserialize");
                    reason = Some(msg.reason);
                }
                ConsensusMessageType::PrepareResponse => {
                    sent_prepare_response = true;
                }
                _ => {}
            }
        }
    }

    assert_eq!(reason, Some(ChangeViewReason::BlockRejectedByPolicy));
    assert!(
        !sent_prepare_response,
        "C# CheckPrepareResponse requests BlockRejectedByPolicy before sending PrepareResponse"
    );
}

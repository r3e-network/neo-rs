use super::*;

#[test]
fn proposal_resolution_caches_unverified_transactions_like_csharp_prepare_request() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = memory_pool(&settings);
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
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = memory_pool(&settings);
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
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = memory_pool(&settings);
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
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = memory_pool(&settings);
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
        &[],
    );

    assert_eq!(hashes, vec![first_hash]);
    assert!(cache.contains_key(&first_hash));
    assert!(
        !cache.contains_key(&second_hash),
        "C# EnsureMaxBlockLimitation breaks before adding the tx that would exceed MaxBlockSystemFee"
    );
}

/// C# v3.10.1 `EnsureMaxBlockLimitation` skips a candidate whose
/// `InvalidTransactions` report count exceeds F (here passed in as the
/// already-thresholded set) — the primary excludes it from the proposal.
#[test]
fn primary_proposal_skips_invalid_transactions_over_f() {
    let settings = ProtocolSettings::default();
    let (validators, _) = consensus_test_validators(4);
    let (private, public, account) = signing_account(0x71);
    let keep = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x7100_0001,
        1,
        0,
        Vec::new(),
    );
    let drop = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        0x7100_0002,
        1,
        0,
        Vec::new(),
    );
    let keep_hash = keep.hash();
    let drop_hash = drop.hash();
    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();

    let hashes = select_primary_proposal_transactions(
        vec![PoolItem::new(keep), PoolItem::new(drop)],
        2,
        &mut cache,
        &validators,
        &settings,
        &[drop_hash],
    );

    assert_eq!(
        hashes,
        vec![keep_hash],
        "the >F-invalid tx is skipped, others kept"
    );
    assert!(cache.contains_key(&keep_hash));
    assert!(!cache.contains_key(&drop_hash));
}

#[test]
fn proposal_resolution_rejects_full_block_over_dbft_max_block_size() {
    let mut settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let pool = memory_pool(&settings);
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
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(memory_pool(&settings));
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
        .await
        .expect("resume backup context");

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let (_tx_feed_tx, tx_feed_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        tx_feed_rx,
        blockchain,
        mempool: pool,
        network,
        settings,
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[1].public_key.clone(),
        store: Arc::new(neo_storage::persistence::providers::memory_store::MemoryStore::new()),
        ledger_provider_factory: test_ledger_provider_factory(),
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
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(memory_pool(&settings));
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
        .await
        .expect("resume backup context");

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let (_tx_feed_tx, tx_feed_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        tx_feed_rx,
        blockchain,
        mempool: pool,
        network,
        settings,
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[1].public_key.clone(),
        store: Arc::new(neo_storage::persistence::providers::memory_store::MemoryStore::new()),
        ledger_provider_factory: test_ledger_provider_factory(),
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

/// C# `ConsensusService.SendPrepareRequest`: right after the primary broadcasts
/// its `PrepareRequest` it announces the proposal transaction hashes via
/// `Inv(TX)` so backups can pull any they lack. The driver's `RequestTransactions`
/// handler must emit that `BroadcastInv(Transaction, hashes)` for the selected
/// proposal transactions.
#[tokio::test]
async fn primary_request_transactions_broadcasts_inv_of_proposal_hashes() {
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(memory_pool(&settings));
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    let tx = signed_zero_fee_tx(&settings, 0x42);
    let tx_hash = tx.hash();
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);

    let (validators, consensus_keys) = consensus_test_validators(4);
    let (event_tx, event_rx) = mpsc::channel(16);
    // Primary for view 0 at block 0 is validator index 0.
    let mut context = ConsensusContext::new(0, validators.clone(), Some(0), Some(1_000));
    context.state = neo_consensus::ConsensusState::Primary;
    let service = ConsensusService::with_context(
        settings.network,
        context,
        consensus_keys[0].to_vec(),
        event_tx,
    );

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, mut network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let (_tx_feed_tx, tx_feed_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        tx_feed_rx,
        blockchain,
        mempool: pool,
        network,
        settings,
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[0].public_key.clone(),
        store: Arc::new(neo_storage::persistence::providers::memory_store::MemoryStore::new()),
        ledger_provider_factory: test_ledger_provider_factory(),
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    driver
        .on_consensus_event(
            ConsensusEvent::RequestTransactions {
                block_index: 0,
                max_count: 512,
                invalid_tx_hashes: Vec::new(),
            },
            &snapshot,
        )
        .await;

    // The primary broadcast an Inv(TX) announcing the proposal transaction.
    let mut inv_hashes = None;
    while let Ok(command) = network_rx.try_recv() {
        if let neo_network::NetworkCommand::BroadcastInv {
            inventory_type,
            hashes,
        } = command
        {
            assert_eq!(inventory_type, neo_network::InventoryType::Transaction);
            inv_hashes = Some(hashes);
            break;
        }
    }
    assert_eq!(
        inv_hashes,
        Some(vec![tx_hash]),
        "primary must announce the proposal transaction hashes via Inv(TX)"
    );
}

/// End-to-end driver wiring for the late-transaction feed (C#
/// `ConsensusService.OnTransaction`): a backup that received a `PrepareRequest`
/// for a transaction it lacked resumes the round — sending its PrepareResponse
/// and caching the transaction body for assembly — when the driver's
/// `tx_feed` arm delivers the transaction after it lands in the mempool.
#[tokio::test]
async fn tx_feed_resumes_backup_and_caches_transaction() {
    let settings = Arc::new(ProtocolSettings::default());
    let snapshot = DataCache::new(false);
    let pool = Arc::new(memory_pool(&settings));
    seed_current_block(&snapshot, 0);
    set_zero_policy_fee(&snapshot, 10);
    set_zero_policy_fee(&snapshot, 18);

    // The proposal references one transaction the backup does not have yet.
    let tx = signed_zero_fee_tx(&settings, 0x43);
    let tx_hash = tx.hash();

    let (validators, consensus_keys) = consensus_test_validators(4);
    let (event_tx, event_rx) = mpsc::channel(16);
    // Backup index 1 that has received a PrepareRequest naming `tx_hash`.
    let mut context = ConsensusContext::new(0, validators.clone(), Some(1), Some(1_000));
    context.prepare_request_received = true;
    context.proposed_tx_hashes = vec![tx_hash];
    let mut service = ConsensusService::with_context(
        settings.network,
        context,
        consensus_keys[1].to_vec(),
        event_tx,
    );
    service
        .resume_with_next_consensus(10_000, UInt256::zero(), UInt160::zero(), 0)
        .await
        .expect("resume backup context");
    // The proposal transaction is still missing → the backup is blocked.
    assert!(service.context().has_missing_proposed_transactions());

    let (blockchain, _blockchain_rx) = BlockchainHandle::with_capacity();
    let (network, _network_rx, _network_events) = NetworkHandle::channel(16, 16);
    let (_inbound_tx, inbound_rx) = mpsc::channel(16);
    let (_tx_feed_tx, tx_feed_rx) = mpsc::channel(16);
    let mut driver = ConsensusDriver {
        service,
        event_rx,
        inbound_rx,
        tx_feed_rx,
        blockchain,
        mempool: Arc::clone(&pool),
        network,
        settings: Arc::clone(&settings),
        validators: Arc::new(RwLock::new(validators.clone())),
        public_key: validators[1].public_key.clone(),
        store: Arc::new(neo_storage::persistence::providers::memory_store::MemoryStore::new()),
        ledger_provider_factory: test_ledger_provider_factory(),
        current_prev_hash: UInt256::default(),
        proposal_txs: HashMap::new(),
    };

    // The transaction propagates and is admitted to the mempool (the backup
    // pulled it via GetData after the primary's Inv), then the driver feeds it.
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    driver.on_transaction_feed(tx_hash).await;

    // The round resumed: the transaction is now available, its body is cached
    // for assembly, and the backup broadcast its PrepareResponse.
    assert!(!driver.service.context().has_missing_proposed_transactions());
    assert!(
        driver.proposal_txs.contains_key(&tx_hash),
        "the fed transaction body is cached for commit-time assembly"
    );

    let mut sent_prepare_response = false;
    while let Ok(event) = driver.event_rx.try_recv() {
        if let ConsensusEvent::BroadcastMessage(payload) = event {
            if payload.message_type == ConsensusMessageType::PrepareResponse {
                sent_prepare_response = true;
            }
        }
    }
    assert!(
        sent_prepare_response,
        "backup resumes and sends its PrepareResponse once the missing tx is fed"
    );
}

/// C# `ConsensusContext._witnessSize` builds the block witness InvocationScript
/// as M pushes of a 64-byte buffer (the M commit signatures). The Rust base
/// block-size estimate must include that invocation — omitting it (an empty
/// invocation) under-counts the base by ~66*M bytes, letting the primary
/// over-pack a near-MaxBlockSize block vs C# and fork.
#[test]
fn expected_base_block_size_includes_commit_signature_invocation() {
    let (validators, _keys) = consensus_test_validators(7);
    let n = validators.len();
    let m = n - (n - 1) / 3; // dBFT M = N - (N-1)/3 = 5 for N = 7
    assert_eq!(m, 5);

    let base = expected_dbft_block_size_without_transactions(0, &validators);

    // Rebuild the witness exactly as C# does: M EmitPush(byte[64]) invocation +
    // the multisig verification script (validator order). `base` must equal this;
    // it would NOT if the invocation were left empty (the prior bug).
    let mut sb = neo_vm::script_builder::ScriptBuilder::new();
    for _ in 0..m {
        sb.emit_push(&[0u8; 64]);
    }
    let invocation = sb.to_array();
    assert_eq!(
        invocation.len(),
        m * 66,
        "each EmitPush(byte[64]) is PUSHDATA1 + len + 64 = 66 bytes"
    );

    let keys: Vec<_> = validators.iter().map(|v| v.public_key.clone()).collect();
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            m, &keys,
        )
        .expect("multisig verification script");

    let header_without_witness = 4 + 32 + 32 + 8 + 8 + 4 + 1 + 20;
    let with_invocation = Witness::new_with_scripts(invocation, verification.clone());
    let expected = header_without_witness
        + 1
        + with_invocation.size()
        + neo_io::serializable::helper::SerializeHelper::get_var_size_usize(0);
    assert_eq!(
        base, expected,
        "base block size must include the M commit-signature invocation"
    );

    // And it must exceed the buggy empty-invocation estimate.
    let empty = Witness::new_with_scripts(Vec::new(), verification);
    let buggy = header_without_witness
        + 1
        + empty.size()
        + neo_io::serializable::helper::SerializeHelper::get_var_size_usize(0);
    assert!(
        base > buggy,
        "fixed base ({base}) must exceed the empty-invocation base ({buggy})"
    );
}

#[test]
fn invalid_validator_set_sizes_as_rejected_proposal() {
    let settings = ProtocolSettings::default();
    let (seed_validators, _keys) = consensus_test_validators(1);
    let seed = &seed_validators[0];
    let validators: Vec<ValidatorInfo> = (0..1025)
        .map(|index| ValidatorInfo {
            index: index as u8,
            public_key: seed.public_key.clone(),
            script_hash: seed.script_hash,
        })
        .collect();

    assert_eq!(
        expected_dbft_block_size_without_transactions(0, &validators),
        usize::MAX,
        "invalid validator sets must fail closed instead of shrinking witness size"
    );

    let tx = signed_zero_fee_tx(&settings, 0x7A);
    let mut cache: HashMap<UInt256, Arc<Transaction>> = HashMap::new();

    let selected = select_primary_proposal_transactions(
        vec![PoolItem::new(tx)],
        1,
        &mut cache,
        &validators,
        &settings,
        &[],
    );

    assert!(selected.is_empty());
    assert!(cache.is_empty());
}

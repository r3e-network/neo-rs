use super::*;
#[test]
fn real_policy_blocked_storage_rejects_system_contract_call_target() {
    crate::install();
    let settings = ProtocolSettings::default();
    let cache = DataCache::new(false);
    let target_hash = UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030405").unwrap();
    deploy_native(&cache, &returning_user_contract(target_hash));
    cache.add(
        PolicyContract::blocked_account_key(&target_hash),
        StorageItem::from_bytes(Vec::new()),
    );

    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push("answer".as_bytes());
    builder.emit_push(&target_hash.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");

    let mut engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::new(cache),
        None,
        settings,
        2000_00000000,
        None,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");

    let state = engine.execute_allow_fault();
    assert_eq!(
        state,
        VmState::FAULT,
        "C# ApplicationEngine.CallContractInternal rejects Policy-blocked contracts before invocation"
    );
    assert_eq!(
        engine.invocation_stack().len(),
        1,
        "blocked contract target must not be loaded as an invocation context"
    );
}

/// Pre-Faun blockAccount (the V0 registration): committee-gated, writes an
/// empty `Prefix_BlockedAccount` record, and double-blocking returns false
/// (C# UT_PolicyContract.Check_BlockAccount).
#[test]
fn block_account_e2e_pre_faun_blocks_then_double_block_returns_false() {
    crate::install();
    // Default MainNet schedules Faun at 8,800,000, so block 0 is pre-Faun.
    let settings = ProtocolSettings::default();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 0),
    );
    let snapshot = Arc::new(cache);
    let signer = committee_address(&committee);
    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

    // blockAccount's pre-Faun path records the persisting block timestamp
    // (Faun onwards stores GetTime()), so the engine needs a persisting
    // block fixture. Height 0 is pre-Faun on MainNet defaults.
    let mut persisting_header = BlockHeader::default();
    persisting_header.set_index(0);
    persisting_header.set_timestamp(1_700_000_000_000);
    let persisting_block = Some(Block::from_parts(persisting_header, vec![]));

    let (state, result) = call_policy(
        Arc::clone(&snapshot),
        signer,
        settings.clone(),
        persisting_block,
        "blockAccount",
        1,
        |b| {
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(state, VmState::HALT, "blockAccount must HALT");
    assert_eq!(result, Some(true), "first block returns true");
    let item = snapshot
        .get(&PolicyContract::blocked_account_key(&account))
        .expect("blocked entry written");
    assert!(
        item.value_bytes().is_empty(),
        "pre-Faun blocked value is empty"
    );

    // Blocking the same account again returns false (no fault).
    let (state2, result2) = call_policy(
        Arc::clone(&snapshot),
        signer,
        settings,
        None,
        "blockAccount",
        1,
        |b| {
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(state2, VmState::HALT, "double block must still HALT");
    assert_eq!(result2, Some(false), "double block returns false");
}

/// blockAccount without the committee witness faults (C# AssertCommittee
/// throws) and writes nothing.
#[test]
fn block_account_e2e_requires_committee_witness() {
    crate::install();
    let settings = ProtocolSettings::default();
    let cache = DataCache::new(false);
    seed_committee(&cache, &sample_committee());
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 0),
    );
    let snapshot = Arc::new(cache);
    let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();

    let (state, _) = call_policy(
        Arc::clone(&snapshot),
        stranger,
        settings,
        None,
        "blockAccount",
        1,
        |b| {
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(
        state,
        VmState::FAULT,
        "non-committee blockAccount must FAULT"
    );
    assert!(
        snapshot
            .get(&PolicyContract::blocked_account_key(&account))
            .is_none()
    );
}

/// blockAccount on a native contract hash faults ("Cannot block a native
/// contract.") even with the committee witness.
#[test]
fn block_account_e2e_rejects_native_contract_hash() {
    crate::install();
    let settings = ProtocolSettings::default();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 0),
    );
    let snapshot = Arc::new(cache);
    let gas_hash = *crate::hashes::GAS_TOKEN_HASH;

    let (state, _) = call_policy(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        None,
        "blockAccount",
        1,
        |b| {
            b.emit_push(&gas_hash.to_array());
        },
    );
    assert_eq!(state, VmState::FAULT, "blocking a native hash must FAULT");
    assert!(
        snapshot
            .get(&PolicyContract::blocked_account_key(&gas_hash))
            .is_none()
    );
}

/// Faun-path blockAccount (the V1 registration): clears the account's vote
/// via NEO.VoteInternal (candidate weight drops, VoteTo cleared,
/// _votersCount reduced) and stamps the blocked entry with the persisting
/// block's millisecond timestamp (`engine.GetTime()`).
#[test]
fn block_account_e2e_faun_clears_vote_and_stamps_time() {
    const BLOCK_TIME_MS: u64 = 1_234_567_890;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );

    // A registered candidate with 100 votes, all from `voter` (balance 100,
    // voting since height 0), and the matching _votersCount.
    let candidate = committee[0].clone();
    let voter = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let candidate_state =
        StackItem::from_struct(vec![StackItem::from_bool(true), StackItem::from_int(100)]);
    let candidate_key = crate::NeoToken::candidate_key(&candidate);
    cache.add(
        candidate_key.clone(),
        StorageItem::from_bytes(
            BinarySerializer::serialize(&candidate_state, &ExecutionEngineLimits::default())
                .unwrap(),
        ),
    );
    let voter_state = StackItem::from_struct(vec![
        StackItem::from_int(100),                          // Balance
        StackItem::from_int(0),                            // BalanceHeight
        StackItem::from_byte_string(candidate.to_bytes()), // VoteTo
        StackItem::from_int(0),                            // LastGasPerVote
    ]);
    let voter_key = crate::NeoToken::account_key(&voter);
    cache.add(
        voter_key.clone(),
        StorageItem::from_bytes(
            BinarySerializer::serialize(&voter_state, &ExecutionEngineLimits::default()).unwrap(),
        ),
    );
    let voters_count_key = crate::NeoToken::voters_count_key();
    cache.add(
        voters_count_key.clone(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(100))),
    );
    let snapshot = Arc::new(cache);

    // Persisting block at index 100 with a known timestamp (GetTime source).
    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(BLOCK_TIME_MS);
    let block = Block::from_parts(header, vec![]);

    let (state, result) = call_policy(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(block),
        "blockAccount",
        1,
        |b| {
            b.emit_push(&voter.to_array());
        },
    );
    assert_eq!(state, VmState::HALT, "Faun blockAccount must HALT");
    assert_eq!(result, Some(true));

    // The blocked entry carries the block timestamp (the recoverFund clock).
    let blocked = snapshot
        .get(&PolicyContract::blocked_account_key(&voter))
        .expect("blocked entry written");
    assert_eq!(
        blocked.value_bytes().into_owned(),
        BigInt::from(BLOCK_TIME_MS).to_signed_bytes_le()
    );

    // The candidate lost the voter's 100-NEO weight (still registered).
    let cand = snapshot.get(&candidate_key).expect("candidate entry kept");
    let decoded =
        BinarySerializer::deserialize(&cand.value_bytes(), &ExecutionEngineLimits::default(), None)
            .unwrap();
    let StackItem::Struct(fields) = decoded else {
        panic!("candidate state is not a struct");
    };
    assert!(
        fields.items()[0].as_bool().unwrap(),
        "candidate stays registered"
    );
    assert_eq!(
        fields.items()[1].as_int().unwrap(),
        BigInt::from(0),
        "votes cleared"
    );

    // The voter's VoteTo is now null and the reward markers advanced.
    let acct = snapshot.get(&voter_key).expect("voter account kept");
    let decoded =
        BinarySerializer::deserialize(&acct.value_bytes(), &ExecutionEngineLimits::default(), None)
            .unwrap();
    let StackItem::Struct(fields) = decoded else {
        panic!("voter account state is not a struct");
    };
    assert_eq!(
        fields.items()[0].as_int().unwrap(),
        BigInt::from(100),
        "balance kept"
    );
    assert!(
        matches!(fields.items()[2], StackItem::Null),
        "VoteTo cleared"
    );

    // _votersCount dropped by the voter's balance (100 -> 0).
    let voters = snapshot.get(&voters_count_key).expect("voters count kept");
    assert_eq!(
        BigInt::from_signed_bytes_le(&voters.value_bytes()),
        BigInt::from(0)
    );
}

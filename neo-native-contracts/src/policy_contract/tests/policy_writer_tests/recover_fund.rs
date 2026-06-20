use super::*;
/// recoverFund's verifiable prefix: the almost-full-committee gate (2-of-3
/// here, max(max(1, n-(n-1)/2), n-2) = 2 for n = 3) plus the
/// "Request not found." fault for an account that was never blocked.
#[test]
fn recover_fund_e2e_requires_request_and_committee() {
    const BLOCK_TIME_MS: u64 = 1_000_000;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );
    let snapshot = Arc::new(cache);
    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
    let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(BLOCK_TIME_MS);

    // Without the almost-full-committee witness -> FAULT.
    let stranger = UInt160::from_bytes(&[0x09; 20]).unwrap();
    let (state, _) = call_policy(
        Arc::clone(&snapshot),
        stranger,
        settings.clone(),
        Some(Block::from_parts(header.clone(), vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
            b.emit_push(&account.to_array()); // account (arg 0, top)
        },
    );
    assert_eq!(
        state,
        VmState::FAULT,
        "non-committee recoverFund must FAULT"
    );

    // With the witness but no blocked entry -> FAULT ("Request not found.").
    // For the 3-member sample committee the almost-full threshold equals the
    // regular committee threshold (both 2-of-3), so the same address signs.
    let (state2, _) = call_policy(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(Block::from_parts(header, vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&gas_hash.to_array());
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(
        state2,
        VmState::FAULT,
        "recoverFund without a request must FAULT"
    );
}

/// Seeds a GAS `AccountState` (`Struct[Balance]`) for `account`.
fn seed_gas_balance(cache: &DataCache, account: &UInt160, balance: i64) {
    let state = StackItem::from_struct(vec![StackItem::from_int(balance)]);
    let key = crate::GasToken::account_key(account);
    cache.add(
        key,
        StorageItem::from_bytes(
            BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap(),
        ),
    );
}

fn seed_blocked_request_time(cache: &DataCache, account: &UInt160, request_time_ms: u64) {
    cache.add(
        PolicyContract::blocked_account_key(account),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            request_time_ms,
        ))),
    );
}

/// recoverFund happy path (C# `PolicyContract.RecoverFund`, lines 663-680):
/// exactly one year after the blocked-account request, an almost-full
/// committee signer sweeps the account's full GAS balance to Treasury
/// through the VM — `balanceOf` then `transfer` issued from the native
/// frame with `account` as the native calling script hash (authorizing the
/// transfer via the `from == CallingScriptHash` bypass), Treasury's
/// `onNEP17Payment` callback included — and emits `Transfer` followed by
/// `RecoveredFund(account)`.
#[test]
fn recover_fund_e2e_sweeps_balance_to_treasury_and_notifies() {
    const REQUEST_TIME_MS: u64 = 1_000_000;
    const SWEPT: i64 = 123_456_789;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );
    deploy_native(
        &cache,
        &build_native_contract_state(&crate::GasToken, &settings, 100),
    );
    // Treasury must be a deployed contract so the GAS transfer's
    // onNEP17Payment callback runs (C# PostTransferAsync calls it whenever
    // the recipient is a contract).
    deploy_native(
        &cache,
        &build_native_contract_state(&crate::Treasury, &settings, 100),
    );

    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
    let treasury = *crate::hashes::TREASURY_HASH;
    let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
    // The blocked-account entry carries the request's millisecond timestamp.
    seed_blocked_request_time(&cache, &account, REQUEST_TIME_MS);
    seed_gas_balance(&cache, &account, SWEPT);
    let snapshot = Arc::new(cache);

    // Exactly one year elapsed: C# faults only when `elapsed < required`,
    // so the boundary block must pass.
    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

    let (state, engine) = call_policy_engine(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(Block::from_parts(header, vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&gas_hash.to_array()); // token (arg 1, deeper)
            b.emit_push(&account.to_array()); // account (arg 0, top)
        },
    );
    assert_eq!(
        state,
        VmState::HALT,
        "recoverFund sweep must HALT: {:?}",
        engine.fault_exception()
    );
    assert!(
        engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
        "recoverFund returns true after a sweep"
    );

    // The full balance moved to Treasury; the account's entry was deleted
    // (an exact-balance NEP-17 transfer removes the from-record).
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &treasury).unwrap(),
        BigInt::from(SWEPT)
    );
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &account).unwrap(),
        BigInt::from(0)
    );
    // recoverFund does not unblock the account.
    assert!(
        snapshot
            .get(&PolicyContract::blocked_account_key(&account))
            .is_some()
    );

    // Notification order matches C#: the GAS Transfer (emitted inside the
    // nested transfer call) first, then Policy's RecoveredFund(account).
    let notifications = engine.notifications();
    assert_eq!(notifications.len(), 2, "expected Transfer + RecoveredFund");
    assert_eq!(notifications[0].script_hash, gas_hash);
    assert_eq!(notifications[0].event_name, "Transfer");
    assert_eq!(
        notifications[0].state[0].as_bytes().unwrap(),
        account.to_bytes()
    );
    assert_eq!(
        notifications[0].state[1].as_bytes().unwrap(),
        treasury.to_bytes()
    );
    assert_eq!(
        notifications[0].state[2].as_int().unwrap(),
        BigInt::from(SWEPT)
    );
    assert_eq!(notifications[1].script_hash, PolicyContract::script_hash());
    assert_eq!(notifications[1].event_name, "RecoveredFund");
    assert_eq!(
        notifications[1].state[0].as_bytes().unwrap(),
        account.to_bytes()
    );
}

/// recoverFund with a zero balance: C# `return false` — HALT, nothing
/// moves, and neither Transfer nor RecoveredFund is emitted.
#[test]
fn recover_fund_e2e_zero_balance_returns_false() {
    const REQUEST_TIME_MS: u64 = 1_000_000;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );
    deploy_native(
        &cache,
        &build_native_contract_state(&crate::GasToken, &settings, 100),
    );

    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
    let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
    seed_blocked_request_time(&cache, &account, REQUEST_TIME_MS);
    let snapshot = Arc::new(cache);

    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

    let (state, engine) = call_policy_engine(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(Block::from_parts(header, vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&gas_hash.to_array());
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(
        state,
        VmState::HALT,
        "zero-balance recoverFund must HALT: {:?}",
        engine.fault_exception()
    );
    assert!(
        !engine.result_stack().peek(0).unwrap().as_bool().unwrap(),
        "recoverFund returns false when there is nothing to sweep"
    );
    assert!(
        engine.notifications().is_empty(),
        "no Transfer/RecoveredFund for an empty sweep"
    );
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &crate::hashes::TREASURY_HASH).unwrap(),
        BigInt::from(0)
    );
}

/// One millisecond short of the one-year window faults (C# "Request must
/// be signed at least 1 year ago. Remaining time: …") and moves no funds.
#[test]
fn recover_fund_e2e_rejects_recent_request() {
    const REQUEST_TIME_MS: u64 = 1_000_000;
    const BALANCE: i64 = 777;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );
    deploy_native(
        &cache,
        &build_native_contract_state(&crate::GasToken, &settings, 100),
    );

    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
    let gas_hash = *crate::hashes::GAS_TOKEN_HASH;
    seed_blocked_request_time(&cache, &account, REQUEST_TIME_MS);
    seed_gas_balance(&cache, &account, BALANCE);
    let snapshot = Arc::new(cache);

    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND - 1);

    let (state, _) = call_policy(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(Block::from_parts(header, vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&gas_hash.to_array());
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(state, VmState::FAULT, "a too-recent request must FAULT");
    assert_eq!(
        crate::GasToken::balance_of(&snapshot, &account).unwrap(),
        BigInt::from(BALANCE),
        "the balance must be untouched"
    );
}

/// A deployed token that does not declare the NEP-17 standard faults (C#
/// "Contract {token} does not implement NEP-17 standard."). Treasury is a
/// deployed non-NEP-17 contract, so it doubles as the token here.
#[test]
fn recover_fund_e2e_requires_nep17_standard() {
    const REQUEST_TIME_MS: u64 = 1_000_000;
    crate::install();
    let settings = faun_settings();
    let cache = DataCache::new(false);
    let committee = sample_committee();
    seed_committee(&cache, &committee);
    deploy_native(
        &cache,
        &build_native_contract_state(&PolicyContract, &settings, 100),
    );
    deploy_native(
        &cache,
        &build_native_contract_state(&crate::Treasury, &settings, 100),
    );

    let account = UInt160::from_bytes(&[0x42; 20]).unwrap();
    let treasury = *crate::hashes::TREASURY_HASH;
    seed_blocked_request_time(&cache, &account, REQUEST_TIME_MS);
    let snapshot = Arc::new(cache);

    let mut header = BlockHeader::default();
    header.set_index(100);
    header.set_timestamp(REQUEST_TIME_MS + REQUIRED_TIME_FOR_RECOVER_FUND);

    let (state, _) = call_policy(
        Arc::clone(&snapshot),
        committee_address(&committee),
        settings,
        Some(Block::from_parts(header, vec![])),
        "recoverFund",
        2,
        &|b| {
            b.emit_push(&treasury.to_array());
            b.emit_push(&account.to_array());
        },
    );
    assert_eq!(state, VmState::FAULT, "a non-NEP-17 token must FAULT");
}

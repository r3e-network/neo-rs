use super::*;
use neo_crypto::signature::Secp256r1Crypto;
use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, OpCode};

/// Deterministic secp256r1 keypair: (private key, SEC1 pubkey,
/// signature-contract script hash).
fn keypair(seed: u8) -> ([u8; 32], Vec<u8>, UInt160) {
    let private = [seed; 32];
    let public = Secp256r1Crypto::derive_public_key(&private).expect("pubkey");
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public);
    (private, public, UInt160::from_script(&script))
}

/// Writes a GAS NEP-17 account record (`Struct[balance]`, the C#
/// `FungibleToken.AccountState`) so the verification balance check
/// passes.
fn mint_gas(snapshot: &DataCache, account: &UInt160, datoshi: i64) {
    let item = StackItem::from_struct(vec![StackItem::from_int(num_bigint::BigInt::from(datoshi))]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
    let mut key = vec![20u8]; // shared NEP-17 Prefix_Account
    key.extend_from_slice(&account.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::GasToken::ID, key),
        neo_storage::StorageItem::from_bytes(bytes),
    );
}

/// Seeds LedgerContract's current-block pointer. C# mempool verification
/// runs against an initialized store, and Ledger.CurrentIndex faults when
/// this item is absent.
fn seed_current_ledger(snapshot: &DataCache, index: u32) {
    let hash = UInt256::from_bytes(&[0u8; 32]).expect("zero hash");
    let bytes = neo_native_contracts::LedgerContract::new()
        .serialize_hash_index_state(&hash, index)
        .expect("hash index state");
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, vec![12]),
        neo_storage::StorageItem::from_bytes(bytes),
    );
}

/// Seeds the Policy settings that C# initializes at genesis and later reads
/// with indexed storage access during transaction verification.
fn seed_policy_fee_settings(snapshot: &DataCache, exec_fee_factor: i64) {
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![10]),
        neo_storage::StorageItem::from_bytes(num_bigint::BigInt::from(1_000).to_signed_bytes_le()),
    );
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::PolicyContract::ID, vec![18]),
        neo_storage::StorageItem::from_bytes(
            num_bigint::BigInt::from(exec_fee_factor).to_signed_bytes_le(),
        ),
    );
}

fn seed_conflict_record(snapshot: &DataCache, hash: &UInt256, signer: &UInt160, index: u32) {
    let stub = neo_native_contracts::LedgerContract::new()
        .serialize_conflict_stub(index)
        .expect("conflict stub");
    let mut bare_key = Vec::with_capacity(33);
    bare_key.push(11);
    bare_key.extend_from_slice(&hash.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, bare_key),
        neo_storage::StorageItem::from_bytes(stub.clone()),
    );

    let mut signer_key = Vec::with_capacity(53);
    signer_key.push(11);
    signer_key.extend_from_slice(&hash.to_bytes());
    signer_key.extend_from_slice(&signer.to_bytes());
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, signer_key),
        neo_storage::StorageItem::from_bytes(stub),
    );
}

/// Builds a properly signed standard single-signature transaction.
fn signed_tx(
    settings: &ProtocolSettings,
    private: &[u8; 32],
    public: &[u8],
    account: UInt160,
    nonce: u32,
    valid_until_block: u32,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    signed_tx_with_fees(
        settings,
        private,
        public,
        account,
        nonce,
        valid_until_block,
        100,
        3_000_000,
        attributes,
    )
}

fn signed_tx_with_fees(
    settings: &ProtocolSettings,
    private: &[u8; 32],
    public: &[u8],
    account: UInt160,
    nonce: u32,
    valid_until_block: u32,
    system_fee: i64,
    network_fee: i64,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_system_fee(system_fee);
    tx.set_network_fee(network_fee); // covers size fee + sig-check cost
    tx.set_valid_until_block(valid_until_block);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_attributes(attributes);
    tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);

    // Sign data = network magic (u32 LE) ‖ tx hash.
    let hash = tx.try_hash().expect("tx hash");
    let mut data = settings.network.to_le_bytes().to_vec();
    data.extend_from_slice(&hash.to_bytes());
    let signature = Secp256r1Crypto::sign(&data, private).expect("sign");

    let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
    invocation.extend_from_slice(&signature);
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(public);
    tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
    tx
}

/// (settings, snapshot-with-funds, keypair) fixture.
fn fixture(seed: u8) -> (ProtocolSettings, DataCache, [u8; 32], Vec<u8>, UInt160) {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);
    let (private, public, account) = keypair(seed);
    seed_current_ledger(&snapshot, 0);
    seed_policy_fee_settings(&snapshot, 30);
    mint_gas(&snapshot, &account, 100_000_000); // 1 GAS
    (settings, snapshot, private, public, account)
}

#[test]
fn empty_pool_has_zero_counts() {
    let pool = MemoryPool::new(&ProtocolSettings::default());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 0);
}

#[test]
fn block_persist_empty_pool_skips_persisted_transaction_scan() {
    let pool = MemoryPool::new(&ProtocolSettings::default());
    let mut oversized = Transaction::new();
    oversized.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
    reset_block_persisted_tx_scan_count();

    let removed = pool.update_pool_for_block_persisted(&[oversized]);

    assert!(removed.is_empty());
    assert_eq!(pool.total_count(), 0);
    assert_eq!(
        block_persisted_tx_scan_count(),
        0,
        "empty mempool fast-sync imports should not hash or inspect block transactions"
    );
}

#[test]
fn valid_signed_transaction_is_admitted_verified() {
    let (settings, snapshot, private, public, account) = fixture(0x42);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 1, 1, Vec::new());
    let hash = tx.hash();
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    assert_eq!(
        pool.verified_count(),
        1,
        "C# TryAdd admits into the sorted pool"
    );
    assert_eq!(pool.unverified_count(), 0);
    assert!(pool.contains(&hash));
}

#[test]
fn post_faun_mempool_divides_stored_exec_fee_factor_like_csharp() {
    let (mut settings, snapshot, private, public, account) = fixture(0x5A);
    settings.hardforks.insert(neo_config::Hardfork::HfFaun, 0);
    seed_policy_fee_settings(
        &snapshot,
        30 * neo_execution::application_engine::FEE_FACTOR,
    );
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 52, 1, Vec::new());

    assert_eq!(
        pool.try_add(tx, &snapshot),
        VerifyResult::Succeed,
        "C# PolicyContract.GetExecFeeFactor(settings, snapshot, height) divides the post-Faun stored pico-GAS factor by ApplicationEngine.FeeFactor"
    );
}

#[test]
fn duplicate_conflicts_attributes_with_same_hash_are_rejected_like_csharp_v3100() {
    let (settings, snapshot, private, public, account) = fixture(0x5B);
    let pool = MemoryPool::new(&settings);
    let absent = UInt256::from([0xA5; 32]);
    let tx = signed_tx(
        &settings,
        &private,
        &public,
        account,
        53,
        1,
        vec![
            TransactionAttribute::Conflicts(neo_payloads::Conflicts::new(absent)),
            TransactionAttribute::Conflicts(neo_payloads::Conflicts::new(absent)),
        ],
    );

    assert_eq!(
        pool.try_add(tx, &snapshot),
        VerifyResult::InvalidAttribute,
        "C# v3.10.0 Conflicts.Verify rejects a transaction carrying duplicate Conflicts attributes for the same hash"
    );
}

#[test]
fn verified_snapshot_returns_highest_fee_first_like_csharp_sorted_reverse() {
    let (settings, snapshot, private, public, account) = fixture(0x43);
    let pool = MemoryPool::new(&settings);
    let low_fee = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        1,
        100,
        100,
        2_000_000,
        Vec::new(),
    );
    let high_fee = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        2,
        100,
        100,
        4_000_000,
        Vec::new(),
    );
    let low_hash = low_fee.hash();
    let high_hash = high_fee.hash();

    assert_eq!(pool.try_add(low_fee, &snapshot), VerifyResult::Succeed);
    assert_eq!(pool.try_add(high_fee, &snapshot), VerifyResult::Succeed);

    let hashes: Vec<UInt256> = pool
        .verified_snapshot()
        .into_iter()
        .map(|item| item.hash())
        .collect();
    assert_eq!(hashes, vec![high_hash, low_hash]);
}

#[test]
fn block_persist_removes_mined_tx_and_evicts_conflicts() {
    let (settings, snapshot, private, public, account) = fixture(0x55);
    let pool = MemoryPool::new(&settings);

    // `mined` is pooled and will be in the block (leg 1: removed).
    let mined = signed_tx(&settings, &private, &public, account, 10, 100, Vec::new());
    // `target` is NOT pooled, only in the block; `conflicting` (pooled,
    // same signer) names it as a Conflicts target (leg 2: evicted on
    // persist because its conflict target becomes on-chain).
    let target = signed_tx(&settings, &private, &public, account, 12, 100, Vec::new());
    let conflicting = signed_tx(
        &settings,
        &private,
        &public,
        account,
        11,
        100,
        vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(target.hash()),
        )],
    );
    assert_eq!(
        pool.try_add(mined.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(conflicting.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(pool.verified_count(), 2);

    // Persist a block containing `mined` and `target`. C#
    // UpdatePoolForBlockPersisted: `mined` is removed (it was confirmed),
    // and `conflicting` is evicted because its Conflicts attribute names the
    // now-persisted `target`.
    let removed = pool.update_pool_for_block_persisted(&[mined.clone(), target.clone()]);
    assert_eq!(
        pool.verified_count(),
        0,
        "both the mined tx and its conflict leave the pool"
    );
    assert!(!pool.contains(&mined.hash()));
    assert!(!pool.contains(&conflicting.hash()));
    assert!(
        removed
            .iter()
            .any(|(tx, reason)| tx.hash() == conflicting.hash()
                && *reason == TransactionRemovalReason::Conflict),
        "the conflicting tx is reported as a Conflict removal"
    );
}

#[test]
fn block_persist_keeps_unverified_conflicts_like_csharp() {
    let (settings, snapshot, private, public, account) = fixture(0x56);
    let pool = MemoryPool::new(&settings);

    let target = signed_tx(&settings, &private, &public, account, 30, 100, Vec::new());
    let conflicting = signed_tx(
        &settings,
        &private,
        &public,
        account,
        31,
        100,
        vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(target.hash()),
        )],
    );
    let conflicting_hash = conflicting.hash();

    assert_eq!(
        pool.try_add(conflicting.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(pool.verified_count(), 1);

    // C# UpdatePoolForBlockPersisted first invalidates verified survivors,
    // moving them to `_unverifiedTransactions`.
    assert!(pool.update_pool_for_block_persisted(&[]).is_empty());
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 1);

    // On the next persisted block, C# scans `_sortedTransactions` only when
    // evicting conflicts with accepted transactions, so an already
    // unverified conflict is not removed at this stage.
    let removed = pool.update_pool_for_block_persisted(&[target]);
    assert!(
        removed.is_empty(),
        "unverified conflicts are left for later reverify like C#"
    );
    assert!(pool.contains(&conflicting_hash));
    assert_eq!(pool.unverified_count(), 1);
}

#[test]
fn block_persist_invalidates_remaining_verified_transactions() {
    let (settings, snapshot, private, public, account) = fixture(0x4E);
    let pool = MemoryPool::new(&settings);
    let first = signed_tx(&settings, &private, &public, account, 20, 1, Vec::new());
    let second = signed_tx(&settings, &private, &public, account, 21, 1, Vec::new());

    assert_eq!(
        pool.try_add(first.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(second.clone(), &snapshot),
        VerifyResult::Succeed
    );
    assert_eq!(pool.verified_count(), 2);
    assert_eq!(pool.unverified_count(), 0);

    let removed = pool.update_pool_for_block_persisted(&[]);
    assert!(removed.is_empty());

    let verified: HashSet<UInt256> = pool
        .verified_snapshot()
        .into_iter()
        .map(|item| item.hash())
        .collect();
    let unverified: HashSet<UInt256> = pool
        .unverified_snapshot()
        .into_iter()
        .map(|item| item.hash())
        .collect();
    assert!(verified.is_empty());
    assert!(unverified.contains(&first.hash()));
    assert!(unverified.contains(&second.hash()));
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 2);
}

#[test]
fn reverify_top_unverified_promotes_highest_priority_survivors() {
    let (settings, snapshot, private, public, account) = fixture(0x78);
    let pool = MemoryPool::new(&settings);
    let low_fee = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        1,
        100,
        100,
        2_000_000,
        Vec::new(),
    );
    let high_fee = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        2,
        100,
        100,
        4_000_000,
        Vec::new(),
    );
    let low_hash = low_fee.hash();
    let high_hash = high_fee.hash();

    assert_eq!(pool.try_add(low_fee, &snapshot), VerifyResult::Succeed);
    assert_eq!(pool.try_add(high_fee, &snapshot), VerifyResult::Succeed);
    pool.update_pool_for_block_persisted(&[]);
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 2);

    assert!(pool.reverify_top_unverified(&snapshot, 1));
    assert_eq!(pool.verified_count(), 1);
    assert_eq!(pool.unverified_count(), 1);
    assert_eq!(pool.verified_snapshot()[0].hash(), high_hash);

    assert!(!pool.reverify_top_unverified(&snapshot, 10));
    let hashes: Vec<UInt256> = pool
        .verified_snapshot()
        .into_iter()
        .map(|item| item.hash())
        .collect();
    assert_eq!(hashes, vec![high_hash, low_hash]);
}

#[test]
fn verified_lookup_does_not_return_unverified_transactions() {
    let (settings, snapshot, private, public, account) = fixture(0x4F);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 22, 1, Vec::new());
    let hash = tx.hash();

    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    assert!(pool.get_verified(&hash).is_some());

    let removed = pool.update_pool_for_block_persisted(&[]);
    assert!(removed.is_empty());
    assert!(pool.get(&hash).is_some());
    assert!(pool.get_verified(&hash).is_none());
}

#[test]
fn duplicate_admission_reports_already_in_pool() {
    let (settings, snapshot, private, public, account) = fixture(0x43);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 2, 1, Vec::new());
    assert_eq!(pool.try_add(tx.clone(), &snapshot), VerifyResult::Succeed);
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::AlreadyInPool);
}

#[test]
fn try_add_conflict_eviction_reports_capacity_exceeded_like_csharp() {
    let (settings, snapshot, private, public, account) = fixture(0x50);
    let mut pool = MemoryPool::new(&settings);
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(
        TransactionRemovalReason,
        Vec<UInt256>,
    )>::new()));
    let captured = events.clone();
    pool.transaction_removed = Some(Box::new(move |_pool, args| {
        captured.lock().unwrap().push((
            args.reason,
            args.transactions.iter().map(|tx| tx.hash()).collect(),
        ));
    }));

    let old = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        30,
        100,
        100,
        3_000_000,
        Vec::new(),
    );
    let replacement = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        31,
        100,
        100,
        6_000_000,
        vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(old.hash()),
        )],
    );

    assert_eq!(pool.try_add(old.clone(), &snapshot), VerifyResult::Succeed);
    assert_eq!(
        pool.try_add(replacement.clone(), &snapshot),
        VerifyResult::Succeed
    );

    assert!(!pool.contains(&old.hash()));
    assert!(pool.contains(&replacement.hash()));
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, TransactionRemovalReason::CapacityExceeded);
    assert_eq!(events[0].1, vec![old.hash()]);
}

#[test]
fn try_add_self_capacity_eviction_fires_added_then_removed_before_out_of_memory() {
    let (mut settings, snapshot, private, public, account) = fixture(0x51);
    settings.memory_pool_max_transactions = 1;
    let mut pool = MemoryPool::new(&settings);
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let added_calls = calls.clone();
    pool.transaction_added = Some(Box::new(move |_pool, tx| {
        added_calls
            .lock()
            .unwrap()
            .push(format!("added:{}", tx.hash()));
    }));
    let removed_calls = calls.clone();
    pool.transaction_removed = Some(Box::new(move |_pool, args| {
        removed_calls.lock().unwrap().push(format!(
            "removed:{:?}:{:?}",
            args.reason,
            args.transactions
                .iter()
                .map(|tx| tx.hash())
                .collect::<Vec<_>>()
        ));
    }));

    let kept = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        32,
        100,
        100,
        6_000_000,
        Vec::new(),
    );
    let evicted = signed_tx_with_fees(
        &settings,
        &private,
        &public,
        account,
        33,
        100,
        100,
        3_000_000,
        Vec::new(),
    );

    assert_eq!(pool.try_add(kept.clone(), &snapshot), VerifyResult::Succeed);
    assert_eq!(
        pool.try_add(evicted.clone(), &snapshot),
        VerifyResult::OutOfMemory
    );

    assert!(pool.contains(&kept.hash()));
    assert!(!pool.contains(&evicted.hash()));
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        [
            format!("added:{}", kept.hash()),
            format!("added:{}", evicted.hash()),
            format!(
                "removed:{:?}:{:?}",
                TransactionRemovalReason::CapacityExceeded,
                vec![evicted.hash()]
            ),
        ]
    );
}

#[test]
fn tampered_signature_reports_invalid_signature() {
    let (settings, snapshot, private, public, account) = fixture(0x44);
    let pool = MemoryPool::new(&settings);
    let mut tx = signed_tx(&settings, &private, &public, account, 3, 1, Vec::new());
    let mut witnesses = tx.witnesses().to_vec();
    *witnesses[0].invocation_script.last_mut().unwrap() ^= 0x01;
    tx.set_witnesses(witnesses);
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidSignature);
}

#[test]
fn expired_transaction_reports_expired() {
    let (settings, snapshot, private, public, account) = fixture(0x45);
    let pool = MemoryPool::new(&settings);
    // C# VerifyStateDependent: ValidUntilBlock <= height (0) → Expired.
    let tx = signed_tx(&settings, &private, &public, account, 4, 0, Vec::new());
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Expired);
}

#[test]
fn too_far_future_valid_until_block_reports_not_yet_valid_like_csharp() {
    let (settings, snapshot, private, public, account) = fixture(0x4f);
    let pool = MemoryPool::new(&settings);
    // C# v3.10.0 Transaction.VerifyStateDependent returns NotYetValid (not
    // Expired) when ValidUntilBlock > height + increment.
    let valid_until_block = settings.max_valid_until_block_increment + 1;
    let tx = signed_tx(
        &settings,
        &private,
        &public,
        account,
        14,
        valid_until_block,
        Vec::new(),
    );
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::NotYetValid);
}

#[test]
fn bad_script_reports_invalid_script() {
    let (settings, snapshot, private, public, account) = fixture(0x46);
    let pool = MemoryPool::new(&settings);
    let mut tx = signed_tx(&settings, &private, &public, account, 5, 1, Vec::new());
    tx.set_script(vec![0xff]); // reserved opcode → strict parse failure
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidScript);
}

#[test]
fn oversize_transaction_reports_oversize() {
    let (settings, snapshot, private, public, account) = fixture(0x47);
    let pool = MemoryPool::new(&settings);
    let mut tx = signed_tx(&settings, &private, &public, account, 6, 1, Vec::new());
    tx.set_script(vec![
        OpCode::PUSH1.byte();
        neo_payloads::MAX_TRANSACTION_SIZE
    ]);
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::OverSize);
}

#[test]
fn blocked_sender_reports_policy_fail() {
    let (settings, snapshot, private, public, account) = fixture(0x48);
    snapshot.add(
        neo_native_contracts::PolicyContract::blocked_account_key(&account),
        neo_storage::StorageItem::from_bytes(Vec::new()),
    );
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 7, 1, Vec::new());
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::PolicyFail);
}

#[test]
fn missing_balance_reports_insufficient_funds() {
    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false); // no GAS minted
    seed_current_ledger(&snapshot, 0);
    let (private, public, account) = keypair(0x49);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 8, 1, Vec::new());
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InsufficientFunds);
}

#[test]
fn not_valid_before_reports_invalid_attribute() {
    let (settings, snapshot, private, public, account) = fixture(0x4A);
    let pool = MemoryPool::new(&settings);
    // NotValidBefore(5) at height 0 → C# NotValidBefore.Verify false.
    let attributes = vec![TransactionAttribute::not_valid_before(5)];
    let tx = signed_tx(&settings, &private, &public, account, 9, 1, attributes);
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::InvalidAttribute);
}

#[test]
fn try_add_does_not_apply_blockchain_conflict_guard_like_csharp() {
    let (settings, snapshot, private, public, account) = fixture(0x5A);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 15, 1, Vec::new());
    seed_conflict_record(&snapshot, &tx.hash(), &account, 0);

    assert_eq!(
        pool.try_add(tx, &snapshot),
        VerifyResult::Succeed,
        "C# MemoryPool.TryAdd assumes Blockchain.OnNewTransaction already applied ContainsConflictHash"
    );
}

#[test]
fn sender_fee_accumulates_until_balance_exhausted() {
    let (settings, snapshot, private, public, account) = fixture(0x4B);
    let pool = MemoryPool::new(&settings);
    // Each tx charges 100 + 3_000_000 against the 100M-datoshi balance.
    // Shrink the balance so only one fits: 2 × 3_000_100 > 4_000_000.
    let mut key = vec![20u8];
    key.extend_from_slice(&account.to_bytes());
    snapshot.delete(&neo_storage::StorageKey::new(
        neo_native_contracts::GasToken::ID,
        key,
    ));
    mint_gas(&snapshot, &account, 4_000_000);
    let first = signed_tx(&settings, &private, &public, account, 10, 1, Vec::new());
    let second = signed_tx(&settings, &private, &public, account, 11, 1, Vec::new());
    assert_eq!(pool.try_add(first, &snapshot), VerifyResult::Succeed);
    assert_eq!(
        pool.try_add(second, &snapshot),
        VerifyResult::InsufficientFunds,
        "pooled sender fees must count against the balance (C# senderFee)"
    );
}

#[test]
fn commit_block_removes_confirmed_and_releases_sender_fee() {
    let (settings, snapshot, private, public, account) = fixture(0x4C);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 12, 1, Vec::new());
    let hash = tx.hash();
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);

    let removed = pool.commit_block(&[hash]);
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].1, TransactionRemovalReason::NoLongerValid);
    assert!(!pool.contains(&hash));

    // The sender-fee reservation is released: a fresh tx fits again.
    let next = signed_tx(&settings, &private, &public, account, 13, 1, Vec::new());
    assert_eq!(pool.try_add(next, &snapshot), VerifyResult::Succeed);
}

#[test]
fn reverify_with_empty_unverified_is_noop() {
    let (settings, snapshot, private, public, account) = fixture(0x4D);
    let pool = MemoryPool::new(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 14, 1, Vec::new());
    assert_eq!(pool.try_add(tx, &snapshot), VerifyResult::Succeed);
    // try_add admits straight into the verified queue (C# TryAdd), so
    // there is nothing to promote.
    let removals = pool.reverify(&snapshot, |_tx, _snap| VerifyResult::Succeed);
    assert!(removals.is_empty());
    assert_eq!(pool.verified_count(), 1);
    assert_eq!(pool.unverified_count(), 0);
}

fn tx_with_signers_and_fees(nonce: u32, sys: i64, net: i64, accounts: &[UInt160]) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_system_fee(sys);
    tx.set_network_fee(net);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(
        accounts
            .iter()
            .map(|a| Signer::new(*a, WitnessScope::NONE))
            .collect(),
    );
    tx.set_witnesses(accounts.iter().map(|_| Witness::empty()).collect());
    tx
}

/// C# `TransactionVerificationContext.CheckTransaction` rebates a conflict's
/// fees only when `conflictTx.Sender == tx.Sender`, and `Sender` is
/// `Signers[0].Account`. A conflict that merely lists the sender as a later
/// (non-first) signer must NOT be rebated.
#[test]
fn conflict_rebate_keys_on_first_signer_like_csharp() {
    let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
    let other = UInt160::from_bytes(&[2u8; 20]).expect("other");

    // (a) first signer IS the sender -> rebated (7 + 3 = 10)
    let first_is_sender = PoolItem::new(tx_with_signers_and_fees(1, 7, 3, &[sender, other]));
    // (b) first signer is someone else, sender appears later -> NOT rebated
    //     (the pre-fix bug rebated this because it matched ANY signer)
    let later_is_sender = PoolItem::new(tx_with_signers_and_fees(2, 100, 100, &[other, sender]));
    // (c) sender absent entirely -> not rebated
    let unrelated = PoolItem::new(tx_with_signers_and_fees(3, 100, 100, &[other]));

    let conflicts = vec![first_is_sender, later_is_sender, unrelated];
    assert_eq!(
        conflict_rebate(&conflicts, Some(sender)),
        num_bigint::BigInt::from(10),
    );
    // No sender -> no rebate.
    assert_eq!(
        conflict_rebate(&conflicts, None),
        num_bigint::BigInt::from(0),
    );
}

use super::*;
use neo_ledger::validation::TransactionVerificationContext;
use neo_native_contracts::AccountState;
use neo_native_contracts::fungible_token::PREFIX_ACCOUNT;
use neo_native_contracts::gas_token::GasToken;
use neo_native_contracts::native_contract::NativeContract;
use neo_primitives::{TransactionRemovalReason, UInt160, UInt256, VerifyResult, WitnessScope};
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::block::Block;
use neo_protocol::payloads::conflicts::Conflicts;
use neo_protocol::payloads::signer::Signer;
use neo_protocol::payloads::transaction::Transaction;
use neo_protocol::payloads::transaction_attribute::TransactionAttribute;
use neo_protocol::payloads::witness::Witness;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::BinarySerializer;
use neo_vm::ExecutionEngineLimits;
use neo_vm::OpCode;
use neo_wallets::KeyPair;
use num_bigint::BigInt;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

fn test_balance_pool(settings: &ProtocolSettings) -> MemoryPool {
    let mut pool = MemoryPool::new(settings);
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));
    pool
}

fn set_gas_balance(snapshot: &DataCache, account: UInt160, amount: i64) {
    let key = StorageKey::create_with_uint160(GasToken::new().id(), PREFIX_ACCOUNT, &account);
    let state = AccountState::with_balance(BigInt::from(amount));
    let bytes = BinarySerializer::serialize_stack_value(
        &state.to_stack_value(),
        &ExecutionEngineLimits::default(),
    )
    .expect("serialize account state");
    snapshot.update(key, StorageItem::from_bytes(bytes));
}

#[test]
fn try_add_rejects_unhashable_transaction() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);
    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

    assert_add_result(&mut pool, tx, &snapshot, &settings, VerifyResult::Invalid);
}

fn build_signed_transaction(
    settings: &ProtocolSettings,
    private_key: [u8; 32],
    network_fee: i64,
    system_fee: i64,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    build_signed_transaction_with_valid_until(
        settings,
        private_key,
        network_fee,
        system_fee,
        attributes,
        1,
    )
}

fn build_signed_transaction_with_valid_until(
    settings: &ProtocolSettings,
    private_key: [u8; 32],
    network_fee: i64,
    system_fee: i64,
    attributes: Vec<TransactionAttribute>,
    valid_until_block: u32,
) -> Transaction {
    let keypair = KeyPair::from_private_key(&private_key).expect("keypair");
    let mut tx = Transaction::new();
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(valid_until_block);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);
    tx.set_attributes(attributes);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
}

fn signed_tx(settings: &ProtocolSettings, key_byte: u8, network_fee: i64) -> Transaction {
    build_signed_transaction(settings, [key_byte; 32], network_fee, 0, Vec::new())
}

fn conflict_attribute(tx: &Transaction) -> TransactionAttribute {
    TransactionAttribute::Conflicts(Conflicts::new(tx.hash()))
}

fn assert_add_result(
    pool: &mut MemoryPool,
    tx: Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    expected: VerifyResult,
) {
    assert_eq!(pool.try_add(tx, snapshot, settings), expected);
}

fn add_succeed(
    pool: &mut MemoryPool,
    tx: Transaction,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
) {
    assert_add_result(pool, tx, snapshot, settings, VerifyResult::Succeed);
}

#[test]
fn new_transaction_event_can_cancel() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);

    let called = Arc::new(AtomicBool::new(false));
    let called_ref = called.clone();
    pool.new_transaction = Some(Box::new(move |_sender, args| {
        called_ref.store(true, AtomicOrdering::SeqCst);
        args.cancel = true;
    }));

    let tx = Transaction::new();
    assert_add_result(
        &mut pool,
        tx,
        &snapshot,
        &settings,
        VerifyResult::PolicyFail,
    );

    assert!(called.load(AtomicOrdering::SeqCst));
}

#[test]
fn transaction_added_event_is_emitted() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let called = Arc::new(AtomicBool::new(false));
    let captured = Arc::new(StdMutex::new(None::<UInt256>));
    let called_ref = called.clone();
    let captured_ref = captured.clone();
    pool.transaction_added = Some(Box::new(move |_sender, tx| {
        called_ref.store(true, AtomicOrdering::SeqCst);
        *captured_ref.lock().unwrap() = Some(tx.hash());
    }));

    let tx = signed_tx(&settings, 1, 1_0000_0000);
    let tx_hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);

    assert!(called.load(AtomicOrdering::SeqCst));
    assert_eq!(captured.lock().unwrap().unwrap(), tx_hash);
}

#[test]
fn capacity_exceeded_emits_removed_event() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 1,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let removed = Arc::new(StdMutex::new(
        Vec::<(TransactionRemovalReason, Vec<UInt256>)>::new(),
    ));
    let removed_ref = removed.clone();
    pool.transaction_removed = Some(Box::new(move |_sender, args| {
        let hashes = args
            .transactions
            .iter()
            .map(|tx| tx.hash())
            .collect::<Vec<_>>();
        removed_ref.lock().unwrap().push((args.reason, hashes));
    }));

    let low_fee = signed_tx(&settings, 2, 1_0000_0000);
    let high_fee = signed_tx(&settings, 3, 3_0000_0000);

    add_succeed(&mut pool, low_fee.clone(), &snapshot, &settings);
    add_succeed(&mut pool, high_fee.clone(), &snapshot, &settings);

    assert!(!pool.contains_key(&low_fee.hash()));
    assert!(pool.contains_key(&high_fee.hash()));

    let captured = removed.lock().unwrap();
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].0, TransactionRemovalReason::CapacityExceeded);
    assert_eq!(captured[0].1, vec![low_fee.hash()]);
}

#[test]
fn try_get_returns_unverified_transactions() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx = signed_tx(&settings, 4, 1_0000_0000);
    add_succeed(&mut pool, tx.clone(), &snapshot, &settings);

    pool.invalidate_verified_transactions();
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 1);

    let fetched = pool.try_get(&tx.hash()).expect("tx");
    assert_eq!(fetched.hash(), tx.hash());
    assert!(pool.contains_key(&tx.hash()));
}

#[test]
fn conflict_with_different_sender_is_rejected() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let base = signed_tx(&settings, 5, 1_0000_0000);
    add_succeed(&mut pool, base.clone(), &snapshot, &settings);

    let conflict = build_signed_transaction(
        &settings,
        [6u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&base)],
    );
    assert_add_result(
        &mut pool,
        conflict,
        &snapshot,
        &settings,
        VerifyResult::HasConflicts,
    );
    assert!(pool.contains_key(&base.hash()));
    assert_eq!(pool.verified_count(), 1);
}

#[test]
fn max_transactions_per_sender_limit_is_enforced() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    pool.set_max_transactions_per_sender(Some(1));
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 101, 1_0000_0000);
    let tx2 = signed_tx(&settings, 101, 2_0000_0000);
    let sender = tx1.sender().expect("sender");

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    assert_add_result(
        &mut pool,
        tx2,
        &snapshot,
        &settings,
        VerifyResult::PolicyFail,
    );
    assert_eq!(pool.sender_transaction_count(&sender), 1);
    assert_eq!(pool.max_transactions_per_sender(), Some(1));
}

#[test]
fn max_transactions_per_sender_allows_conflict_replacement() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    pool.set_max_transactions_per_sender(Some(1));
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 102, 1_0000_0000);
    let tx2 = build_signed_transaction(
        &settings,
        [102u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&tx1)],
    );
    let sender = tx1.sender().expect("sender");

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    assert!(!pool.contains_key(&tx1.hash()));
    assert!(pool.contains_key(&tx2.hash()));
    assert_eq!(pool.sender_transaction_count(&sender), 1);
}

#[test]
fn conflict_replacement_emits_conflict_reason() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let removed = Arc::new(StdMutex::new(Vec::new()));
    let removed_ref = Arc::clone(&removed);
    pool.transaction_removed = Some(Box::new(move |_sender, args| {
        removed_ref.lock().unwrap().push((
            args.reason,
            args.transactions
                .iter()
                .map(|tx| tx.hash())
                .collect::<Vec<_>>(),
        ));
    }));

    let original = signed_tx(&settings, 103, 1_0000_0000);
    let replacement = build_signed_transaction(
        &settings,
        [103u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&original)],
    );
    add_succeed(&mut pool, original.clone(), &snapshot, &settings);
    add_succeed(&mut pool, replacement, &snapshot, &settings);

    let removed = removed.lock().unwrap();
    assert_eq!(removed.len(), 1);
    // Conflict replacement evicts the displaced transaction because it
    // conflicts, not because the pool is over capacity (audit D-04,
    // 2026-09-08: `CapacityExceeded` here was an incidental drift from the
    // v0.15.0 baseline; the genuine capacity path asserts its own reason).
    assert_eq!(removed[0].0, TransactionRemovalReason::Conflict);
    assert_eq!(removed[0].1, vec![original.hash()]);
}

#[test]
fn higher_fee_conflict_replaces_multiple_transactions() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 7, 1_0000_0000);
    let tx2 = signed_tx(&settings, 7, 1_0000_0000);
    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    let replacement = build_signed_transaction(
        &settings,
        [7u8; 32],
        2_0000_0000 + 1,
        0,
        vec![conflict_attribute(&tx1), conflict_attribute(&tx2)],
    );
    add_succeed(&mut pool, replacement.clone(), &snapshot, &settings);

    assert!(pool.contains_key(&replacement.hash()));
    assert!(!pool.contains_key(&tx1.hash()));
    assert!(!pool.contains_key(&tx2.hash()));
    assert_eq!(pool.verified_count(), 1);
}

#[test]
fn update_pool_for_block_persisted_keeps_conflict_without_shared_signer() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let pool_tx = signed_tx(&settings, 8, 1_0000_0000);
    add_succeed(&mut pool, pool_tx.clone(), &snapshot, &settings);

    let block_tx = build_signed_transaction(
        &settings,
        [9u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&pool_tx)],
    );

    let mut block = Block::new();
    block.header.set_index(1);
    block.transactions = vec![block_tx];

    pool.update_pool_for_block_persisted(&block, &snapshot, &settings, true);

    assert!(pool.contains_key(&pool_tx.hash()));
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 1);
}

#[test]
fn capacity_enforces_high_fee_eviction() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 2,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 20, 1_0000_0000);
    let tx_mid = signed_tx(&settings, 21, 2_0000_0000);
    let tx_high = signed_tx(&settings, 22, 3_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_mid.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);

    assert_eq!(pool.count(), 2);
    assert!(!pool.contains_key(&tx_low.hash()));
    assert!(pool.contains_key(&tx_mid.hash()));
    assert!(pool.contains_key(&tx_high.hash()));
}

#[test]
fn sorted_verified_transactions_respects_limit_and_order() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 10, 1_0000_0000);
    let tx_mid = signed_tx(&settings, 11, 2_0000_0000);
    let tx_high = signed_tx(&settings, 12, 3_0000_0000);

    add_succeed(&mut pool, tx_low, &snapshot, &settings);
    add_succeed(&mut pool, tx_mid.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);

    let top_two = pool.sorted_verified_transactions(2);
    assert_eq!(top_two.len(), 2);
    assert_eq!(top_two[0].hash(), tx_high.hash());
    assert_eq!(top_two[1].hash(), tx_mid.hash());
}

#[test]
fn sorted_verified_transactions_subset_is_consistent() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 40, 1_0000_0000);
    let tx_mid = signed_tx(&settings, 41, 2_0000_0000);
    let tx_high = signed_tx(&settings, 42, 3_0000_0000);
    let tx_top = signed_tx(&settings, 43, 4_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_mid.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_top.clone(), &snapshot, &settings);

    let top_two = pool.sorted_verified_transactions(2);
    let top_four = pool.sorted_verified_transactions(4);
    assert_eq!(top_two.len(), 2);
    assert_eq!(top_four.len(), 4);
    assert_eq!(top_two[0].hash(), top_four[0].hash());
    assert_eq!(top_two[1].hash(), top_four[1].hash());
    assert_eq!(top_four[0].hash(), tx_top.hash());
    assert_eq!(top_four[1].hash(), tx_high.hash());
    assert_eq!(top_four[2].hash(), tx_mid.hash());
    assert_eq!(top_four[3].hash(), tx_low.hash());
}

#[test]
fn can_transaction_fit_in_pool_checks_lowest_fee() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 3,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let low = signed_tx(&settings, 30, 1_0000_0000);
    let mid = signed_tx(&settings, 31, 2_0000_0000);
    let high = signed_tx(&settings, 32, 3_0000_0000);

    add_succeed(&mut pool, low, &snapshot, &settings);
    add_succeed(&mut pool, mid, &snapshot, &settings);
    add_succeed(&mut pool, high, &snapshot, &settings);

    let lower = signed_tx(&settings, 33, 1_0000_0000 - 1);
    assert!(!pool.can_transaction_fit_in_pool(&lower));

    let higher = signed_tx(&settings, 34, 1_0000_0000 + 1);
    assert!(pool.can_transaction_fit_in_pool(&higher));
}

#[test]
fn reverify_promotes_highest_fee_first() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 13, 1_0000_0000);
    let tx_mid = signed_tx(&settings, 14, 2_0000_0000);
    let tx_high = signed_tx(&settings, 15, 3_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_mid.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);

    pool.invalidate_verified_transactions();
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 3);
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    let still_pending = pool.reverify_top_unverified_transactions(1, &snapshot, &settings, false);
    assert!(still_pending);
    assert_eq!(pool.verified_count(), 1);

    let verified = pool.sorted_verified_transactions(1);
    assert_eq!(verified.len(), 1);
    assert_eq!(verified[0].hash(), tx_high.hash());
}

#[test]
fn reverify_batches_progress_without_exhausting_pool() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 1000,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    for i in 0..100u8 {
        let fee = 1_0000_0000 + (i as i64) * 10_000;
        let key = 40u8.wrapping_add(i);
        let tx = signed_tx(&settings, key, fee);
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }

    pool.invalidate_verified_transactions();
    pool._max_milliseconds_to_reverify_tx_per_idle = 0.0;
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 100);

    let still_pending = pool.reverify_top_unverified_transactions(20, &snapshot, &settings, false);
    assert!(still_pending);
    assert_eq!(pool.verified_count(), 20);
    assert_eq!(pool.unverified_count(), 80);

    let still_pending = pool.reverify_top_unverified_transactions(30, &snapshot, &settings, false);
    assert!(still_pending);
    assert_eq!(pool.verified_count(), 50);
    assert_eq!(pool.unverified_count(), 50);

    let verified = pool.sorted_verified_transactions(50);
    for pair in verified.windows(2) {
        assert!(pair[0].fee_per_byte() >= pair[1].fee_per_byte());
    }
}

#[test]
fn update_pool_with_header_backlog_moves_to_unverified() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 30, 1_0000_0000);
    let tx2 = signed_tx(&settings, 31, 1_1000_0000);
    let tx3 = signed_tx(&settings, 32, 1_2000_0000);

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx3.clone(), &snapshot, &settings);

    let mut block = Block::new();
    block.header.set_index(1);
    block.transactions = vec![tx1.clone()];

    pool.update_pool_for_block_persisted(&block, &snapshot, &settings, true);

    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 2);
    assert!(!pool.contains_key(&tx1.hash()));
    assert!(pool.contains_key(&tx2.hash()));
    assert!(pool.contains_key(&tx3.hash()));
}

#[test]
fn invalidate_all_clears_pool() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 40, 1_0000_0000);
    let tx2 = signed_tx(&settings, 41, 1_1000_0000);

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    pool.invalidate_all_transactions();
    assert_eq!(pool.count(), 0);
    assert!(!pool.contains_key(&tx1.hash()));
    assert!(!pool.contains_key(&tx2.hash()));
}

#[test]
fn contains_key_tracks_verified_and_unverified() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx = signed_tx(&settings, 50, 1_0000_0000);
    add_succeed(&mut pool, tx.clone(), &snapshot, &settings);
    assert!(pool.contains_key(&tx.hash()));

    pool.invalidate_verified_transactions();
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 1);
    assert!(pool.contains_key(&tx.hash()));
}

#[test]
fn iterator_returns_verified_and_unverified_transactions() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 10, 1_0000_0000);
    let tx2 = signed_tx(&settings, 11, 2_0000_0000);
    let tx3 = signed_tx(&settings, 12, 3_0000_0000);

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    add_succeed(&mut pool, tx3.clone(), &snapshot, &settings);

    let all = pool.all_transactions_vec();
    assert_eq!(all.len(), 3);
    let hashes: HashSet<UInt256> = all.iter().map(|tx| tx.hash()).collect();
    assert!(hashes.contains(&tx1.hash()));
    assert!(hashes.contains(&tx2.hash()));
    assert!(hashes.contains(&tx3.hash()));

    let iter_hashes: HashSet<UInt256> = (&pool).into_iter().map(|tx| tx.hash()).collect();
    assert_eq!(iter_hashes.len(), 3);
    assert!(iter_hashes.contains(&tx1.hash()));
    assert!(iter_hashes.contains(&tx2.hash()));
    assert!(iter_hashes.contains(&tx3.hash()));
}

#[test]
fn verified_and_unverified_transactions_are_sorted_descending() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 20, 1_0000_0000);
    let tx_mid = signed_tx(&settings, 21, 2_0000_0000);
    let tx_high = signed_tx(&settings, 22, 3_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_mid.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);

    let (verified, unverified) = pool.verified_and_unverified_transactions();
    assert_eq!(verified.len(), 3);
    assert!(unverified.is_empty());
    assert_eq!(verified[0].hash(), tx_high.hash());
    assert_eq!(verified[1].hash(), tx_mid.hash());
    assert_eq!(verified[2].hash(), tx_low.hash());

    pool.invalidate_verified_transactions();
    let (verified, unverified) = pool.verified_and_unverified_transactions();
    assert!(verified.is_empty());
    assert_eq!(unverified.len(), 3);
    assert_eq!(unverified[0].hash(), tx_high.hash());
    assert_eq!(unverified[1].hash(), tx_mid.hash());
    assert_eq!(unverified[2].hash(), tx_low.hash());
}

#[test]
fn try_add_rejects_duplicate_transactions() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx = signed_tx(&settings, 60, 1_0000_0000);
    add_succeed(&mut pool, tx.clone(), &snapshot, &settings);
    assert_add_result(
        &mut pool,
        tx,
        &snapshot,
        &settings,
        VerifyResult::AlreadyInPool,
    );
}

#[test]
fn conflict_chain_rejects_lower_fee_replacement() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 70, 1_0000_0000);
    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);

    let tx2 = build_signed_transaction(
        &settings,
        [70u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&tx1)],
    );
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    let tx3 = build_signed_transaction(
        &settings,
        [70u8; 32],
        1_5000_0000,
        0,
        vec![conflict_attribute(&tx2)],
    );
    assert_add_result(
        &mut pool,
        tx3,
        &snapshot,
        &settings,
        VerifyResult::HasConflicts,
    );

    assert_eq!(pool.count(), 1);
    assert!(pool.contains_key(&tx2.hash()));
    assert!(!pool.contains_key(&tx1.hash()));
}

#[test]
fn conflict_chain_allows_nonexistent_conflict_replacement() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 80, 1_0000_0000);
    let tx2 = build_signed_transaction(
        &settings,
        [80u8; 32],
        2_0000_0000,
        0,
        vec![conflict_attribute(&tx1)],
    );
    let tx3 = build_signed_transaction(
        &settings,
        [80u8; 32],
        1_5000_0000,
        0,
        vec![conflict_attribute(&tx2)],
    );
    let tx4 = build_signed_transaction(
        &settings,
        [80u8; 32],
        3_0000_0000,
        0,
        vec![conflict_attribute(&tx3)],
    );

    add_succeed(&mut pool, tx1, &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);
    assert_add_result(
        &mut pool,
        tx3,
        &snapshot,
        &settings,
        VerifyResult::HasConflicts,
    );
    add_succeed(&mut pool, tx4.clone(), &snapshot, &settings);

    assert_eq!(pool.count(), 2);
    assert!(pool.contains_key(&tx2.hash()));
    assert!(pool.contains_key(&tx4.hash()));
}

#[test]
fn block_persist_removes_one_same_payer_fee_only_once() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let private_key = [201u8; 32];

    let tx1 = build_signed_transaction_with_valid_until(
        &settings,
        private_key,
        1_0000_0000,
        0,
        Vec::new(),
        100,
    );
    let tx2 = build_signed_transaction_with_valid_until(
        &settings,
        private_key,
        2_0000_0000,
        0,
        Vec::new(),
        100,
    );
    let sender = tx1.sender().expect("sender");
    assert_eq!(tx2.sender(), Some(sender));
    set_gas_balance(&snapshot, sender, 50_0000_0000);

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);
    assert_eq!(
        pool.verification_context
            .total_fee_for_sender(&sender)
            .cloned(),
        Some(BigInt::from(3_0000_0000i64))
    );

    let mut block = Block::new();
    block.header.set_index(1);
    block.transactions = vec![tx1.clone()];

    pool.update_pool_for_block_persisted(&block, &snapshot, &settings, false);

    assert_eq!(pool.verified_count(), 1);
    assert_eq!(pool.unverified_count(), 0);
    assert_eq!(
        pool.verification_context
            .total_fee_for_sender(&sender)
            .cloned(),
        Some(BigInt::from(2_0000_0000i64))
    );
    assert!(pool.contains_key(&tx2.hash()));
}

#[test]
fn block_persist_moves_to_unverified_and_reverify() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);

    let mut txs = Vec::new();
    for i in 0..70u8 {
        let key = i.saturating_add(1);
        let tx = signed_tx(&settings, key, 1_0000_0000);
        if let Some(sender) = tx.sender() {
            set_gas_balance(&snapshot, sender, 1_0000_0000);
        }
        add_succeed(&mut pool, tx.clone(), &snapshot, &settings);
        txs.push(tx);
    }

    assert_eq!(pool.sorted_tx_count(), 70);

    let mut block = Block::new();
    block.header.set_index(1);
    block.transactions = txs[..10].to_vec();

    pool.update_pool_for_block_persisted(&block, &snapshot, &settings, true);

    assert_eq!(pool.sorted_tx_count(), 0);
    assert_eq!(pool.unverified_sorted_tx_count(), 60);

    for step in 1..=6 {
        pool.reverify_top_unverified_transactions(10, &snapshot, &settings, false);
        assert_eq!(pool.sorted_tx_count(), 10 * step);
        assert_eq!(pool.unverified_sorted_tx_count(), 60 - 10 * step);
    }
}

#[test]
fn block_persist_reverify_drops_transactions_after_balance_change() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);

    let mut txs = Vec::new();
    for i in 0..70u8 {
        let key = i.saturating_add(1);
        let tx = signed_tx(&settings, key, 1_0000_0000);
        if let Some(sender) = tx.sender() {
            set_gas_balance(&snapshot, sender, 1_0000_0000);
        }
        add_succeed(&mut pool, tx.clone(), &snapshot, &settings);
        txs.push(tx);
    }

    let mut block = Block::new();
    block.header.set_index(1);
    block.transactions = txs[..10].to_vec();

    for (idx, tx) in txs.iter().enumerate().skip(10) {
        let balance = if idx < 40 { 1_0000_0000 } else { 0 };
        if let Some(sender) = tx.sender() {
            set_gas_balance(&snapshot, sender, balance);
        }
    }

    pool.update_pool_for_block_persisted(&block, &snapshot, &settings, false);

    assert_eq!(pool.sorted_tx_count(), 30);
    assert_eq!(pool.unverified_sorted_tx_count(), 0);
}

#[test]
fn unverified_high_priority_transactions_prevent_low_fee_admission() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 100,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    for i in 0..99u8 {
        let tx = signed_tx(&settings, 50u8 + i, 5_0000_0000);
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }

    pool.invalidate_verified_transactions();
    assert_eq!(pool.unverified_count(), 99);
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));
    assert!(pool.can_transaction_fit_in_pool(&signed_tx(&settings, 10, 5_0000_0000)));

    let tx = signed_tx(&settings, 20, 5_0000_0000);
    add_succeed(&mut pool, tx, &snapshot, &settings);
    assert_eq!(pool.count(), 100);

    let low = signed_tx(&settings, 21, 1_0000_0000);
    assert!(!pool.can_transaction_fit_in_pool(&low));
}

#[test]
fn verified_transactions_vec_returns_only_verified() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = signed_tx(&settings, 60, 1_0000_0000);
    let tx2 = signed_tx(&settings, 61, 2_0000_0000);

    add_succeed(&mut pool, tx1.clone(), &snapshot, &settings);
    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));
    add_succeed(&mut pool, tx2.clone(), &snapshot, &settings);

    let verified = pool.verified_transactions_vec();
    assert_eq!(verified.len(), 1);
    assert_eq!(verified[0].hash(), tx2.hash());
}

#[test]
fn reverify_uses_requested_count_when_verified_exceeds_max_per_block() {
    let settings = ProtocolSettings {
        max_transactions_per_block: 2,
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    for i in 0..3u8 {
        let tx = signed_tx(&settings, 70u8 + i, 1_0000_0000);
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }

    let unverified_1 = signed_tx(&settings, 80, 2_0000_0000);
    let unverified_2 = signed_tx(&settings, 81, 1_0000_0000);
    pool.insert_unverified_for_test(unverified_1);
    pool.insert_unverified_for_test(unverified_2);

    assert_eq!(pool.verified_count(), 3);
    assert_eq!(pool.unverified_count(), 2);

    let still_pending = pool.reverify_top_unverified_transactions(2, &snapshot, &settings, false);
    assert!(!still_pending);
    assert_eq!(pool.verified_count(), 5);
    assert_eq!(pool.unverified_count(), 0);
}

#[test]
fn invalidate_verified_resets_state_even_when_verified_is_empty() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let tx = signed_tx(&settings, 82, 1_0000_0000);
    let hash = tx.hash();
    let sender = tx.sender().expect("sender");
    let conflicting_hash = signed_tx(&settings, 83, 2_0000_0000).hash();

    pool.conflicts
        .insert(hash, HashSet::from([conflicting_hash]));
    pool.verification_context.add_transaction(&tx);

    assert_eq!(pool.verified_count(), 0);
    assert!(pool.conflicts.contains_key(&hash));
    assert!(
        pool.verification_context
            .total_fee_for_sender(&sender)
            .is_some()
    );

    pool.invalidate_verified_transactions();

    assert!(pool.conflicts.is_empty());
    assert!(
        pool.verification_context
            .total_fee_for_sender(&sender)
            .is_none()
    );
}

#[test]
fn try_add_handles_multi_conflict_scenarios() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let sender_key = [90u8; 32];
    let malicious_key = [91u8; 32];

    let mp1 = build_signed_transaction(&settings, sender_key, 1_0000_0000, 0, Vec::new());
    let mp2_1 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![conflict_attribute(&mp1)],
    );
    let mp2_2 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![conflict_attribute(&mp1)],
    );
    add_succeed(&mut pool, mp2_1.clone(), &snapshot, &settings);
    add_succeed(&mut pool, mp2_2.clone(), &snapshot, &settings);

    let mp3 = build_signed_transaction(
        &settings,
        sender_key,
        2_0000_0000,
        0,
        vec![conflict_attribute(&mp1)],
    );
    add_succeed(&mut pool, mp3.clone(), &snapshot, &settings);

    assert_add_result(
        &mut pool,
        mp1.clone(),
        &snapshot,
        &settings,
        VerifyResult::HasConflicts,
    );
    assert!(pool.contains_key(&mp3.hash()));

    let malicious = build_signed_transaction(
        &settings,
        malicious_key,
        3_0000_0000,
        0,
        vec![conflict_attribute(&mp3)],
    );
    assert_add_result(
        &mut pool,
        malicious,
        &snapshot,
        &settings,
        VerifyResult::HasConflicts,
    );
    assert!(pool.contains_key(&mp3.hash()));

    let mp4 = build_signed_transaction(
        &settings,
        sender_key,
        3_0000_0000,
        0,
        vec![conflict_attribute(&mp3)],
    );
    add_succeed(&mut pool, mp4.clone(), &snapshot, &settings);
    assert!(pool.contains_key(&mp4.hash()));
    assert!(!pool.contains_key(&mp3.hash()));

    let mp6 = build_signed_transaction(
        &settings,
        sender_key,
        mp2_1.network_fee() + mp2_2.network_fee() + 1,
        0,
        vec![conflict_attribute(&mp2_1), conflict_attribute(&mp2_2)],
    );
    add_succeed(&mut pool, mp6.clone(), &snapshot, &settings);
    assert!(pool.contains_key(&mp6.hash()));
    assert!(!pool.contains_key(&mp2_1.hash()));
    assert!(!pool.contains_key(&mp2_2.hash()));

    let mp7 = build_signed_transaction(&settings, sender_key, 2_0000_0000 + 1, 0, Vec::new());
    let mp8 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![conflict_attribute(&mp7)],
    );
    let mp9 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![conflict_attribute(&mp7)],
    );
    let mp10 = build_signed_transaction(
        &settings,
        malicious_key,
        1_0000_0000,
        0,
        vec![conflict_attribute(&mp7)],
    );

    add_succeed(&mut pool, mp8.clone(), &snapshot, &settings);
    add_succeed(&mut pool, mp9.clone(), &snapshot, &settings);
    add_succeed(&mut pool, mp10.clone(), &snapshot, &settings);
    add_succeed(&mut pool, mp7.clone(), &snapshot, &settings);

    assert!(pool.contains_key(&mp7.hash()));
    assert!(!pool.contains_key(&mp8.hash()));
    assert!(!pool.contains_key(&mp9.hash()));
    assert!(!pool.contains_key(&mp10.hash()));
}

#[test]
fn reverify_restores_higher_fee_conflict() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);

    let sender_key = [100u8; 32];
    let mp1 = build_signed_transaction(&settings, sender_key, 1_0000_0000, 0, Vec::new());
    let mp2 = build_signed_transaction(
        &settings,
        sender_key,
        2_0000_0000,
        0,
        vec![conflict_attribute(&mp1)],
    );

    if let Some(sender) = mp1.sender() {
        set_gas_balance(&snapshot, sender, 1_0000_0000);
    }

    add_succeed(&mut pool, mp1.clone(), &snapshot, &settings);

    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    // Adding a higher fee conflict during reverify should succeed
    add_succeed(&mut pool, mp2.clone(), &snapshot, &settings);

    // Reverify should handle the conflicts correctly
    pool.reverify_top_unverified_transactions(10, &snapshot, &settings, false);

    assert!(pool.contains_key(&mp2.hash()));
}

#[test]
fn iter_verified_returns_transactions_in_priority_order() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 10, 1_0000_0000);
    let tx_high = signed_tx(&settings, 11, 3_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);

    let collected: Vec<_> = pool.iter_verified().collect();
    assert_eq!(collected.len(), 2);
    assert_eq!(collected[0].hash(), tx_high.hash());
    assert_eq!(collected[1].hash(), tx_low.hash());
}

#[test]
fn iter_unverified_returns_transactions_in_priority_order() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = signed_tx(&settings, 10, 1_0000_0000);
    let tx_high = signed_tx(&settings, 11, 3_0000_0000);

    add_succeed(&mut pool, tx_low.clone(), &snapshot, &settings);
    add_succeed(&mut pool, tx_high.clone(), &snapshot, &settings);
    pool.invalidate_verified_transactions();

    let collected: Vec<_> = pool.iter_unverified().collect();
    assert_eq!(collected.len(), 2);
    assert_eq!(collected[0].hash(), tx_high.hash());
    assert_eq!(collected[1].hash(), tx_low.hash());
}

#[test]
fn arc_transaction_returns_correct_data() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx = signed_tx(&settings, 1, 1_0000_0000);
    let tx_hash = tx.hash();
    let tx_network_fee = tx.network_fee();

    add_succeed(&mut pool, tx, &snapshot, &settings);

    let arc_tx = pool.try_get(&tx_hash).expect("transaction should exist");
    assert_eq!(arc_tx.hash(), tx_hash);
    assert_eq!(arc_tx.network_fee(), tx_network_fee);

    // Arc should allow multiple references without cloning
    let arc_tx2 = pool.try_get(&tx_hash).expect("transaction should exist");
    assert!(Arc::ptr_eq(&arc_tx, &arc_tx2) || arc_tx.hash() == arc_tx2.hash());
}

fn quarantine_metadata(hash: UInt256, now: u64) -> QuarantineMetadata {
    QuarantineMetadata {
        transaction_hash: hash,
        evidence_refs: vec!["fixed-probe-rule-1".to_string()],
        context: "trusted double replay finding".to_string(),
        created_at_unix_ms: now,
        expires_at_unix_ms: now + super::quarantine::QUARANTINE_TTL_MS,
        rule_version: "restricted-probe-result-v1".to_string(),
        episode: 0,
        cumulative_active_ms: 0,
    }
}

fn enable_quarantine(pool: &mut MemoryPool, max_entries: usize, scan_budget: usize) {
    pool.configure_quarantine(
        QuarantineConfig {
            automatic_enabled: true,
            max_entries,
            max_ttl_ms: super::quarantine::QUARANTINE_TTL_MS,
            selection_scan_budget: scan_budget,
        },
        1_000,
    )
    .expect("enable quarantine");
}

fn quarantine_tx(settings: &ProtocolSettings, key: u16, network_fee: i64) -> Transaction {
    let mut private_key = [0u8; 32];
    private_key[..2].copy_from_slice(&key.to_le_bytes());
    build_signed_transaction(settings, private_key, network_fee, 0, Vec::new())
}

fn add_quarantine_support(
    pool: &mut MemoryPool,
    snapshot: &DataCache,
    settings: &ProtocolSettings,
    key: u16,
) {
    let tx = quarantine_tx(settings, key, 1_0000_0000);
    add_succeed(pool, tx, snapshot, settings);
}

#[test]
fn automatic_quarantine_is_disabled_by_default() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 110, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);

    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000),
        Err(QuarantineError::Disabled)
    );
    assert_eq!(pool.quarantine_count(1_000), 0);
}

#[test]
fn disabled_quarantine_keeps_canonical_proposal_order_and_limit() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let high = signed_tx(&settings, 126, 3_0000_0000);
    let middle = signed_tx(&settings, 127, 2_0000_0000);
    let low = signed_tx(&settings, 128, 1_0000_0000);
    add_succeed(&mut pool, high.clone(), &snapshot, &settings);
    add_succeed(&mut pool, middle, &snapshot, &settings);
    add_succeed(&mut pool, low, &snapshot, &settings);

    pool.configure_quarantine(
        QuarantineConfig {
            automatic_enabled: false,
            max_entries: 4,
            max_ttl_ms: super::quarantine::QUARANTINE_TTL_MS,
            selection_scan_budget: 1,
        },
        1_000,
    )
    .expect("disabled config");

    let selected = pool.local_proposal_transactions(2, 1_000);
    assert_eq!(selected.len(), 2);
    assert_eq!(
        selected.iter().map(|tx| tx.hash()).collect::<Vec<_>>(),
        pool.get_sorted_verified_transactions(2)
            .iter()
            .map(|tx| tx.hash())
            .collect::<Vec<_>>()
    );
    assert_eq!(selected[0].hash(), high.hash());
}

#[test]
fn quarantine_metadata_does_not_replace_transaction_state() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 111, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 911);
    enable_quarantine(&mut pool, 4, 2_048);

    pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000)
        .expect("quarantine");

    assert!(pool.contains_key(&hash));
    assert_eq!(pool.verified_count(), 2);
    assert!(pool.try_get(&hash).is_some());
    assert_eq!(pool.quarantine_count(1_100), 1);
}

#[test]
fn local_proposal_filter_backfills_but_external_views_ignore_labels() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let high = signed_tx(&settings, 112, 3_0000_0000);
    let middle = signed_tx(&settings, 113, 2_0000_0000);
    let low = signed_tx(&settings, 114, 1_0000_0000);
    add_succeed(&mut pool, high.clone(), &snapshot, &settings);
    add_succeed(&mut pool, middle.clone(), &snapshot, &settings);
    add_succeed(&mut pool, low.clone(), &snapshot, &settings);
    pool.configure_quarantine(
        QuarantineConfig {
            automatic_enabled: true,
            max_entries: 4,
            max_ttl_ms: super::quarantine::QUARANTINE_TTL_MS,
            selection_scan_budget: 3,
        },
        1_000,
    )
    .expect("config");
    pool.quarantine_transaction(quarantine_metadata(high.hash(), 1_000), 1_000)
        .expect("quarantine");

    let local = pool.local_proposal_transactions(2, 1_100);
    assert_eq!(local.len(), 2);
    assert!(!local.iter().any(|tx| tx.hash() == high.hash()));
    assert!(local.iter().any(|tx| tx.hash() == middle.hash()));
    assert!(local.iter().any(|tx| tx.hash() == low.hash()));

    let external = pool.verified_transactions_vec();
    assert_eq!(external.len(), 3);
    assert!(external.iter().any(|tx| tx.hash() == high.hash()));
    assert!(pool.try_get(&high.hash()).is_some());
    assert!(pool.contains_key(&high.hash()));
    assert!(
        pool.sorted_verified_transactions(3)
            .iter()
            .any(|tx| tx.hash() == high.hash())
    );
    assert!(
        pool.all_transactions_vec()
            .iter()
            .any(|tx| tx.hash() == high.hash())
    );
}

#[test]
fn quarantine_ttl_expires_automatically_for_local_selection() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 115, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 915);
    enable_quarantine(&mut pool, 4, 2_048);
    pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000)
        .expect("quarantine");

    assert!(
        !pool
            .local_proposal_transactions(2, 30_999)
            .iter()
            .any(|transaction| transaction.hash() == hash)
    );
    let selected = pool.local_proposal_transactions(2, 31_000);
    assert!(
        selected
            .iter()
            .any(|transaction| transaction.hash() == hash)
    );
    assert_eq!(pool.quarantine_count(31_000), 0);
}

#[test]
fn active_quarantine_label_cannot_renew() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 116, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 916);
    enable_quarantine(&mut pool, 4, 2_048);

    pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000)
        .expect("first episode");
    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hash, 2_000), 2_000),
        Err(QuarantineError::AlreadyActive)
    );
    let metadata = pool.quarantine_metadata(&hash, 2_000).expect("first label");
    assert_eq!(metadata.expires_at_unix_ms, 31_000);
    assert_eq!(metadata.episode, 1);
}

#[test]
fn quarantine_episode_cap_is_two() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 119, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 919);
    enable_quarantine(&mut pool, 4, 2_048);

    let mut first = quarantine_metadata(hash, 1_000);
    first.expires_at_unix_ms = 16_000;
    pool.quarantine_transaction(first, 1_000)
        .expect("first episode");
    let mut second = quarantine_metadata(hash, 16_000);
    second.expires_at_unix_ms = 31_000;
    pool.quarantine_transaction(second, 16_000)
        .expect("second episode");
    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hash, 31_000), 31_000),
        Err(QuarantineError::LifetimeExhausted)
    );
}

#[test]
fn quarantine_cumulative_lifetime_is_capped_at_sixty_seconds() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 120, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 920);
    enable_quarantine(&mut pool, 4, 2_048);
    pool.quarantine_history.insert(
        hash,
        super::quarantine::QuarantineHistory {
            episodes: 1,
            cumulative_ms: 50_000,
        },
    );

    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000),
        Err(QuarantineError::LifetimeExhausted)
    );
}

#[test]
fn context_and_rule_changes_do_not_reset_quarantine_history() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 121, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 921);
    enable_quarantine(&mut pool, 4, 2_048);

    pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000)
        .expect("first episode");
    assert_eq!(pool.take_quarantine_expiry_reviews(31_000, 1).len(), 1);
    let mut second = quarantine_metadata(hash, 31_000);
    second.context = "changed context must not reset lifetime".to_string();
    second.rule_version = "restricted-probe-result-v2".to_string();
    pool.quarantine_transaction(second, 31_000)
        .expect("second episode");
    let metadata = pool
        .quarantine_metadata(&hash, 31_000)
        .expect("second label");
    assert_eq!(metadata.episode, 2);
    assert_eq!(metadata.cumulative_active_ms, 60_000);
}

#[test]
fn expiry_restores_eligibility_and_schedules_bounded_review() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 117, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 917);
    enable_quarantine(&mut pool, 4, 2_048);
    pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000)
        .expect("quarantine");

    assert!(
        !pool
            .local_proposal_transactions(2, 30_999)
            .iter()
            .any(|transaction| transaction.hash() == hash)
    );
    assert!(
        pool.local_proposal_transactions(2, 31_000)
            .iter()
            .any(|transaction| transaction.hash() == hash)
    );
    let reviews = pool.take_quarantine_expiry_reviews(31_000, 8);
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].transaction_hash, hash);
    assert_eq!(reviews[0].expired_at_unix_ms, 31_000);
    assert!(pool.take_quarantine_expiry_reviews(31_000, 8).is_empty());
}

#[test]
fn expiry_review_queue_evicts_oldest_at_its_bound() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let transactions = [
        signed_tx(&settings, 123, 3_0000_0000),
        signed_tx(&settings, 124, 2_0000_0000),
        signed_tx(&settings, 125, 1_0000_0000),
    ];
    let hashes = transactions.each_ref().map(Transaction::hash);
    for transaction in transactions {
        add_succeed(&mut pool, transaction, &snapshot, &settings);
    }
    enable_quarantine(&mut pool, 1, 2_048);

    pool.quarantine_transaction(quarantine_metadata(hashes[0], 1_000), 1_000)
        .expect("first episode");
    pool.quarantine_transaction(quarantine_metadata(hashes[1], 31_000), 31_000)
        .expect("second hash");
    pool.quarantine_transaction(quarantine_metadata(hashes[2], 61_000), 61_000)
        .expect("third hash");
    let reviews = pool.take_quarantine_expiry_reviews(91_000, usize::MAX);
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].transaction_hash, hashes[2]);
}

#[test]
fn small_pool_cannot_be_fully_quarantined() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 122, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    enable_quarantine(&mut pool, 4, 2_048);

    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hash, 1_000), 1_000),
        Err(QuarantineError::ImpactLimitReached)
    );
    assert_eq!(pool.local_proposal_transactions(1, 1_000)[0].hash(), hash);
}

#[test]
fn quarantine_impact_is_limited_to_one_percent_of_pool() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 128,
        ..ProtocolSettings::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let mut hashes = Vec::new();
    for key in 200u16..301 {
        let tx = quarantine_tx(&settings, key, 100_000_000 + i64::from(key) * 100_000);
        set_gas_balance(
            &snapshot,
            tx.sender().expect("signed transaction sender"),
            5_000_000_000,
        );
        hashes.push(tx.hash());
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }
    assert_eq!(pool.verified_count(), 101);
    enable_quarantine(&mut pool, 1_024, 2_048);
    pool.quarantine_transaction(quarantine_metadata(hashes[0], 1_000), 1_000)
        .expect("one percent allows one");
    assert_eq!(
        pool.quarantine_transaction(quarantine_metadata(hashes[1], 1_000), 1_000),
        Err(QuarantineError::ImpactLimitReached)
    );
}

#[test]
fn quarantine_ttl_is_clamped_to_thirty_seconds() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let tx = signed_tx(&settings, 118, 3_0000_0000);
    let hash = tx.hash();
    add_succeed(&mut pool, tx, &snapshot, &settings);
    add_quarantine_support(&mut pool, &snapshot, &settings, 918);
    enable_quarantine(&mut pool, 4, 2_048);
    let mut metadata = quarantine_metadata(hash, 1_000);
    metadata.expires_at_unix_ms = 91_000;
    pool.quarantine_transaction(metadata, 1_000)
        .expect("overlong request must be clamped");
    let stored = pool
        .quarantine_metadata(&hash, 1_000)
        .expect("active metadata");
    assert_eq!(stored.expires_at_unix_ms, 31_000);
    assert_eq!(stored.cumulative_active_ms, 30_000);
}

#[test]
fn local_proposal_scan_is_strictly_budgeted_and_advances_by_inspected_count() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 512,
        ..ProtocolSettings::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    for key in 120u16..420 {
        let tx = quarantine_tx(&settings, key, 100_000_000 + i64::from(key) * 100_000);
        set_gas_balance(
            &snapshot,
            tx.sender().expect("signed transaction sender"),
            5_000_000_000,
        );
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }
    pool.configure_quarantine(
        QuarantineConfig {
            automatic_enabled: true,
            max_entries: 10,
            max_ttl_ms: super::quarantine::QUARANTINE_TTL_MS,
            selection_scan_budget: 3,
        },
        1_000,
    )
    .expect("config");
    let first_three = pool.get_sorted_verified_transactions(3);
    let expected_first_cursor = first_three[2].hash();
    for tx in &first_three {
        pool.quarantine_transaction(quarantine_metadata(tx.hash(), 1_000), 1_000)
            .expect("quarantine");
    }

    assert!(pool.local_proposal_transactions(5, 1_100).is_empty());
    assert_eq!(pool.proposal_last_scan_count, 3);
    assert_eq!(
        pool.proposal_scan_cursor
            .as_ref()
            .map(|item| item.transaction.hash()),
        Some(expected_first_cursor)
    );
    let next = pool.local_proposal_transactions(1, 1_100);
    assert_eq!(next.len(), 1);
    assert_eq!(pool.proposal_last_scan_count, 1);
    assert_eq!(
        pool.proposal_scan_cursor
            .as_ref()
            .map(|item| item.transaction.hash()),
        Some(next[0].hash())
    );
}

#[test]
fn large_pool_rotation_eventually_selects_every_interval() {
    let settings = ProtocolSettings {
        memory_pool_max_transactions: 128,
        ..ProtocolSettings::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);
    let mut expected = HashSet::new();
    for key in 1..=80 {
        let tx = signed_tx(&settings, key, 100_000_000 + i64::from(key) * 100_000);
        set_gas_balance(
            &snapshot,
            tx.sender().expect("signed transaction sender"),
            5_000_000_000,
        );
        expected.insert(tx.hash());
        add_succeed(&mut pool, tx, &snapshot, &settings);
    }
    assert_eq!(pool.verified_count(), expected.len());
    pool.configure_quarantine(
        QuarantineConfig {
            automatic_enabled: true,
            max_entries: 8,
            max_ttl_ms: super::quarantine::QUARANTINE_TTL_MS,
            selection_scan_budget: 7,
        },
        1_000,
    )
    .expect("config");

    let mut observed = HashSet::new();
    for _ in 0..expected.len() {
        let selected = pool.local_proposal_transactions(1, 1_100);
        assert_eq!(selected.len(), 1);
        assert!(pool.proposal_last_scan_count <= 7);
        observed.insert(selected[0].hash());
    }
    assert_eq!(
        observed, expected,
        "rotation must cover the entire real pool"
    );
}

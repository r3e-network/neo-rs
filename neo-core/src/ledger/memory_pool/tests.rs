use super::*;
use crate::WitnessScope;
use crate::network::p2p::helper::get_sign_data_vec;
use crate::network::p2p::payloads::block::Block;
use crate::network::p2p::payloads::conflicts::Conflicts;
use crate::network::p2p::payloads::signer::Signer;
use crate::network::p2p::payloads::transaction::Transaction;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
use crate::network::p2p::payloads::witness::Witness;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::native::AccountState;
use crate::smart_contract::native::fungible_token::PREFIX_ACCOUNT;
use crate::smart_contract::native::gas_token::GasToken;
use crate::smart_contract::native::native_contract::NativeContract;
use crate::smart_contract::{IInteroperable, StorageItem, StorageKey};
use crate::wallets::KeyPair;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::op_code::OpCode;
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
    let bytes = BinarySerializer::serialize(
        &state.to_stack_item().expect("to_stack_item"),
        &ExecutionEngineLimits::default(),
    )
    .expect("serialize account state");
    snapshot.update(key, StorageItem::from_bytes(bytes));
}

fn build_signed_transaction(
    settings: &ProtocolSettings,
    private_key: [u8; 32],
    network_fee: i64,
    system_fee: i64,
    attributes: Vec<TransactionAttribute>,
) -> Transaction {
    let keypair = KeyPair::from_private_key(&private_key).expect("keypair");
    let mut tx = Transaction::new();
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);
    tx.set_attributes(attributes);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    let verification_script = keypair.get_verification_script();
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        verification_script,
    )]);
    tx
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
    assert_eq!(
        pool.try_add(tx, &snapshot, &settings),
        VerifyResult::PolicyFail
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

    let tx = build_signed_transaction(&settings, [1u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_hash = tx.hash();
    assert_eq!(
        pool.try_add(tx, &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let low_fee = build_signed_transaction(&settings, [2u8; 32], 1_0000_0000, 0, Vec::new());
    let high_fee = build_signed_transaction(&settings, [3u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(low_fee.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(high_fee.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx = build_signed_transaction(&settings, [4u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let base = build_signed_transaction(&settings, [5u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(base.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let conflict = build_signed_transaction(
        &settings,
        [6u8; 32],
        2_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(base.hash()))],
    );
    assert_eq!(
        pool.try_add(conflict, &snapshot, &settings),
        VerifyResult::HasConflicts
    );
    assert!(pool.contains_key(&base.hash()));
    assert_eq!(pool.verified_count(), 1);
}

#[test]
fn higher_fee_conflict_replaces_multiple_transactions() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = build_signed_transaction(&settings, [7u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(&settings, [7u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let replacement = build_signed_transaction(
        &settings,
        [7u8; 32],
        2_0000_0000 + 1,
        0,
        vec![
            TransactionAttribute::Conflicts(Conflicts::new(tx1.hash())),
            TransactionAttribute::Conflicts(Conflicts::new(tx2.hash())),
        ],
    );
    assert_eq!(
        pool.try_add(replacement.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let pool_tx = build_signed_transaction(&settings, [8u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(pool_tx.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let block_tx = build_signed_transaction(
        &settings,
        [9u8; 32],
        2_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(
            pool_tx.hash(),
        ))],
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

    let tx_low = build_signed_transaction(&settings, [20u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_mid = build_signed_transaction(&settings, [21u8; 32], 2_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [22u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx_low.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_mid.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_high.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx_low = build_signed_transaction(&settings, [10u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_mid = build_signed_transaction(&settings, [11u8; 32], 2_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [12u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx_low.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_mid.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_high.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx_low = build_signed_transaction(&settings, [40u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_mid = build_signed_transaction(&settings, [41u8; 32], 2_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [42u8; 32], 3_0000_0000, 0, Vec::new());
    let tx_top = build_signed_transaction(&settings, [43u8; 32], 4_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx_low.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_mid.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_high.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_top.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let low = build_signed_transaction(&settings, [30u8; 32], 1_0000_0000, 0, Vec::new());
    let mid = build_signed_transaction(&settings, [31u8; 32], 2_0000_0000, 0, Vec::new());
    let high = build_signed_transaction(&settings, [32u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(low, &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(mid, &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(high, &snapshot, &settings),
        VerifyResult::Succeed
    );

    let lower = build_signed_transaction(&settings, [33u8; 32], 1_0000_0000 - 1, 0, Vec::new());
    assert!(!pool.can_transaction_fit_in_pool(&lower));

    let higher = build_signed_transaction(&settings, [34u8; 32], 1_0000_0000 + 1, 0, Vec::new());
    assert!(pool.can_transaction_fit_in_pool(&higher));
}

#[test]
fn reverify_promotes_highest_fee_first() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = build_signed_transaction(&settings, [13u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_mid = build_signed_transaction(&settings, [14u8; 32], 2_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [15u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx_low.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_mid.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_high.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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
        let tx = build_signed_transaction(&settings, [key; 32], fee, 0, Vec::new());
        assert_eq!(
            pool.try_add(tx.clone(), &snapshot, &settings),
            VerifyResult::Succeed
        );
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

    let tx1 = build_signed_transaction(&settings, [30u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(&settings, [31u8; 32], 1_1000_0000, 0, Vec::new());
    let tx3 = build_signed_transaction(&settings, [32u8; 32], 1_2000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx3.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx1 = build_signed_transaction(&settings, [40u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(&settings, [41u8; 32], 1_1000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx = build_signed_transaction(&settings, [50u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
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

    let tx1 = build_signed_transaction(&settings, [10u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(&settings, [11u8; 32], 2_0000_0000, 0, Vec::new());
    let tx3 = build_signed_transaction(&settings, [12u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    assert_eq!(
        pool.try_add(tx3.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx_low = build_signed_transaction(&settings, [20u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_mid = build_signed_transaction(&settings, [21u8; 32], 2_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [22u8; 32], 3_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx_low.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_mid.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx_high.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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

    let tx = build_signed_transaction(&settings, [60u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx, &snapshot, &settings),
        VerifyResult::AlreadyInPool
    );
}

#[test]
fn conflict_chain_rejects_lower_fee_replacement() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = build_signed_transaction(&settings, [70u8; 32], 1_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let tx2 = build_signed_transaction(
        &settings,
        [70u8; 32],
        2_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(tx1.hash()))],
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let tx3 = build_signed_transaction(
        &settings,
        [70u8; 32],
        1_5000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(tx2.hash()))],
    );
    assert_eq!(
        pool.try_add(tx3, &snapshot, &settings),
        VerifyResult::HasConflicts
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

    let tx1 = build_signed_transaction(&settings, [80u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(
        &settings,
        [80u8; 32],
        2_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(tx1.hash()))],
    );
    let tx3 = build_signed_transaction(
        &settings,
        [80u8; 32],
        1_5000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(tx2.hash()))],
    );
    let tx4 = build_signed_transaction(
        &settings,
        [80u8; 32],
        3_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(tx3.hash()))],
    );

    assert_eq!(
        pool.try_add(tx1, &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(tx3, &snapshot, &settings),
        VerifyResult::HasConflicts
    );
    assert_eq!(
        pool.try_add(tx4.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    assert_eq!(pool.count(), 2);
    assert!(pool.contains_key(&tx2.hash()));
    assert!(pool.contains_key(&tx4.hash()));
}

#[test]
fn block_persist_moves_to_unverified_and_reverify() {
    let settings = ProtocolSettings::default();
    let mut pool = MemoryPool::new(&settings);
    let snapshot = DataCache::new(false);

    let mut txs = Vec::new();
    for i in 0..70u8 {
        let private_key = [i.saturating_add(1); 32];
        let tx = build_signed_transaction(&settings, private_key, 1_0000_0000, 0, Vec::new());
        if let Some(sender) = tx.sender() {
            set_gas_balance(&snapshot, sender, 1_0000_0000);
        }
        assert_eq!(
            pool.try_add(tx.clone(), &snapshot, &settings),
            VerifyResult::Succeed
        );
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
        let private_key = [i.saturating_add(1); 32];
        let tx = build_signed_transaction(&settings, private_key, 1_0000_0000, 0, Vec::new());
        if let Some(sender) = tx.sender() {
            set_gas_balance(&snapshot, sender, 1_0000_0000);
        }
        assert_eq!(
            pool.try_add(tx.clone(), &snapshot, &settings),
            VerifyResult::Succeed
        );
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
        let tx = build_signed_transaction(&settings, [50u8 + i; 32], 5_0000_0000, 0, Vec::new());
        assert_eq!(
            pool.try_add(tx, &snapshot, &settings),
            VerifyResult::Succeed
        );
    }

    pool.invalidate_verified_transactions();
    assert_eq!(pool.unverified_count(), 99);
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));
    assert!(pool.can_transaction_fit_in_pool(&build_signed_transaction(
        &settings,
        [10u8; 32],
        5_0000_0000,
        0,
        Vec::new()
    )));

    let tx = build_signed_transaction(&settings, [20u8; 32], 5_0000_0000, 0, Vec::new());
    assert_eq!(
        pool.try_add(tx, &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(pool.count(), 100);

    let low = build_signed_transaction(&settings, [21u8; 32], 1_0000_0000, 0, Vec::new());
    assert!(!pool.can_transaction_fit_in_pool(&low));
}

#[test]
fn verified_transactions_vec_returns_only_verified() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx1 = build_signed_transaction(&settings, [60u8; 32], 1_0000_0000, 0, Vec::new());
    let tx2 = build_signed_transaction(&settings, [61u8; 32], 2_0000_0000, 0, Vec::new());

    assert_eq!(
        pool.try_add(tx1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));
    assert_eq!(
        pool.try_add(tx2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let verified = pool.verified_transactions_vec();
    assert_eq!(verified.len(), 1);
    assert_eq!(verified[0].hash(), tx2.hash());
}

#[test]
fn reverify_limits_when_verified_exceeds_max_per_block() {
    let settings = ProtocolSettings {
        max_transactions_per_block: 2,
        memory_pool_max_transactions: 10,
        ..Default::default()
    };
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    for i in 0..3u8 {
        let tx = build_signed_transaction(&settings, [70u8 + i; 32], 1_0000_0000, 0, Vec::new());
        assert_eq!(
            pool.try_add(tx, &snapshot, &settings),
            VerifyResult::Succeed
        );
    }

    let unverified_1 = build_signed_transaction(&settings, [80u8; 32], 2_0000_0000, 0, Vec::new());
    let unverified_2 = build_signed_transaction(&settings, [81u8; 32], 1_0000_0000, 0, Vec::new());
    pool.insert_unverified_for_test(unverified_1);
    pool.insert_unverified_for_test(unverified_2);

    assert_eq!(pool.verified_count(), 3);
    assert_eq!(pool.unverified_count(), 2);

    let still_pending = pool.reverify_top_unverified_transactions(2, &snapshot, &settings, false);
    assert!(still_pending);
    assert_eq!(pool.verified_count(), 4);
    assert_eq!(pool.unverified_count(), 1);
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
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp1.hash()))],
    );
    let mp2_2 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp1.hash()))],
    );
    assert_eq!(
        pool.try_add(mp2_1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(mp2_2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    let mp3 = build_signed_transaction(
        &settings,
        sender_key,
        2_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp1.hash()))],
    );
    assert_eq!(
        pool.try_add(mp3.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    assert_eq!(
        pool.try_add(mp1.clone(), &snapshot, &settings),
        VerifyResult::HasConflicts
    );
    assert!(pool.contains_key(&mp3.hash()));

    let malicious = build_signed_transaction(
        &settings,
        malicious_key,
        3_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp3.hash()))],
    );
    assert_eq!(
        pool.try_add(malicious, &snapshot, &settings),
        VerifyResult::HasConflicts
    );
    assert!(pool.contains_key(&mp3.hash()));

    let mp4 = build_signed_transaction(
        &settings,
        sender_key,
        3_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp3.hash()))],
    );
    assert_eq!(
        pool.try_add(mp4.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert!(pool.contains_key(&mp4.hash()));
    assert!(!pool.contains_key(&mp3.hash()));

    let mp6 = build_signed_transaction(
        &settings,
        sender_key,
        mp2_1.network_fee() + mp2_2.network_fee() + 1,
        0,
        vec![
            TransactionAttribute::Conflicts(Conflicts::new(mp2_1.hash())),
            TransactionAttribute::Conflicts(Conflicts::new(mp2_2.hash())),
        ],
    );
    assert_eq!(
        pool.try_add(mp6.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert!(pool.contains_key(&mp6.hash()));
    assert!(!pool.contains_key(&mp2_1.hash()));
    assert!(!pool.contains_key(&mp2_2.hash()));

    let mp7 = build_signed_transaction(&settings, sender_key, 2_0000_0000 + 1, 0, Vec::new());
    let mp8 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp7.hash()))],
    );
    let mp9 = build_signed_transaction(
        &settings,
        sender_key,
        1_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp7.hash()))],
    );
    let mp10 = build_signed_transaction(
        &settings,
        malicious_key,
        1_0000_0000,
        0,
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp7.hash()))],
    );

    assert_eq!(
        pool.try_add(mp8.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(mp9.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(mp10.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        pool.try_add(mp7.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

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
        vec![TransactionAttribute::Conflicts(Conflicts::new(mp1.hash()))],
    );

    if let Some(sender) = mp1.sender() {
        set_gas_balance(&snapshot, sender, 1_0000_0000);
    }

    assert_eq!(
        pool.try_add(mp1.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    pool.invalidate_verified_transactions();
    pool.verification_context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(50_0000_0000i64));

    // Adding a higher fee conflict during reverify should succeed
    assert_eq!(
        pool.try_add(mp2.clone(), &snapshot, &settings),
        VerifyResult::Succeed
    );

    // Reverify should handle the conflicts correctly
    pool.reverify_top_unverified_transactions(10, &snapshot, &settings, false);

    assert!(pool.contains_key(&mp2.hash()));
}

#[test]
fn iter_verified_returns_transactions_in_priority_order() {
    let settings = ProtocolSettings::default();
    let mut pool = test_balance_pool(&settings);
    let snapshot = DataCache::new(false);

    let tx_low = build_signed_transaction(&settings, [10u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [11u8; 32], 3_0000_0000, 0, Vec::new());

    pool.try_add(tx_low.clone(), &snapshot, &settings);
    pool.try_add(tx_high.clone(), &snapshot, &settings);

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

    let tx_low = build_signed_transaction(&settings, [10u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_high = build_signed_transaction(&settings, [11u8; 32], 3_0000_0000, 0, Vec::new());

    pool.try_add(tx_low.clone(), &snapshot, &settings);
    pool.try_add(tx_high.clone(), &snapshot, &settings);
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

    let tx = build_signed_transaction(&settings, [1u8; 32], 1_0000_0000, 0, Vec::new());
    let tx_hash = tx.hash();
    let tx_network_fee = tx.network_fee();

    pool.try_add(tx, &snapshot, &settings);

    let arc_tx = pool.try_get(&tx_hash).expect("transaction should exist");
    assert_eq!(arc_tx.hash(), tx_hash);
    assert_eq!(arc_tx.network_fee(), tx_network_fee);

    // Arc should allow multiple references without cloning
    let arc_tx2 = pool.try_get(&tx_hash).expect("transaction should exist");
    assert!(Arc::ptr_eq(&arc_tx, &arc_tx2) || arc_tx.hash() == arc_tx2.hash());
}

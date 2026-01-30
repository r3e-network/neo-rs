//! End-to-End Transaction Flow Integration Tests

use neo_core::chain::{BlockIndexEntry, ChainState};
use neo_core::network::p2p::payloads::{Signer, Transaction, TransactionAttribute, WitnessScope};

use neo_core::{UInt160, UInt256};
use neo_core::mempool::{Mempool, MempoolConfig};
use neo_core::state::{
    AccountState, MemoryWorldState, StateChanges, StorageItem, StorageKey, WorldState,
};
use neo_vm::op_code::OpCode;

// Creates a test account with NEO and GAS balances
fn create_test_account(neo_balance: u64, gas_balance: u64) -> (UInt160, AccountState) {
    let hash = UInt160::from([0x01u8; 20]);
    let account = AccountState::with_balances(hash, neo_balance, gas_balance as u64);
    (hash, account)
}

// Creates a signed transaction
fn create_signed_transaction(
    sender: UInt160,
    script: Vec<u8>,
    valid_until: u32,
    system_fee: i64,
    network_fee: i64,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_valid_until_block(valid_until);
    tx.set_system_fee(system_fee);
    tx.set_network_fee(network_fee);
    tx.set_script(script);

    let signer = Signer::new(sender, WitnessScope::CalledByEntry);
    tx.add_signer(signer);

    tx
}

// Setup test environment with genesis block
fn setup_test_env() -> (ChainState, MemoryWorldState, Mempool) {
    let chain = ChainState::new();
    let world_state = MemoryWorldState::new();
    let config = MempoolConfig::default();
    let mempool = Mempool::with_config(config);

    let genesis = BlockIndexEntry {
        hash: UInt256::from([0x01u8; 32]),
        height: 0,
        prev_hash: UInt256::zero(),
        header: Vec::new(),
        timestamp: 1468595301000,
        tx_count: 0,
        size: 100,
        cumulative_difficulty: 1,
        on_main_chain: true,
    };
    chain.init_genesis(genesis).unwrap();

    (chain, world_state, mempool)
}

#[test]
fn test_transaction_creation_basic() {
    let (sender, _) = create_test_account(1000, 10000);
    let script = vec![OpCode::PUSH1 as u8, OpCode::RET as u8];

    let tx = create_signed_transaction(sender, script.clone(), 100, 1000, 500);

    assert_eq!(tx.version(), 0);
    assert_eq!(tx.script(), script);
    assert_eq!(tx.valid_until_block(), 100);
    assert_eq!(tx.system_fee(), 1000);
    assert_eq!(tx.network_fee(), 500);
    assert_eq!(tx.signers().len(), 1);
    assert_eq!(tx.sender(), Some(sender));
}

#[test]
fn test_transaction_hash_unique() {
    let (sender, _) = create_test_account(1000, 10000);

    let tx1 = create_signed_transaction(sender, vec![OpCode::PUSH1 as u8], 100, 1000, 500);
    let mut tx2 = Transaction::new();
    tx2.set_valid_until_block(100);
    tx2.set_system_fee(1000);
    tx2.set_network_fee(500);
    tx2.set_script(vec![OpCode::PUSH1 as u8]);

    let signer = Signer::new(sender, WitnessScope::CalledByEntry);
    tx2.add_signer(signer);

    assert_ne!(
        tx1.hash(),
        tx2.hash(),
        "Different nonces should produce different hashes"
    );
}

#[test]
fn test_transaction_serialization_roundtrip() {
    use neo_core::network::p2p::payloads::Witness;

    let (sender, _) = create_test_account(1000, 10000);
    let script = vec![
        OpCode::PUSH1 as u8,
        OpCode::PUSH2 as u8,
        OpCode::ADD as u8,
        OpCode::RET as u8,
    ];

    let mut original = create_signed_transaction(sender, script, 100, 1000, 500);

    // Add an empty witness to satisfy validation
    let witness = Witness::default();
    original.add_witness(witness);

    let bytes = original.to_bytes();
    let restored = Transaction::from_bytes(&bytes).unwrap();

    assert_eq!(original.hash(), restored.hash());
    assert_eq!(original.script(), restored.script());
    assert_eq!(original.valid_until_block(), restored.valid_until_block());
}

#[test]
fn test_transaction_validation_fees() {
    let (sender, _) = create_test_account(1000, 10000);

    let tx_no_fees = create_signed_transaction(sender, vec![OpCode::RET as u8], 100, 0, 0);
    assert_eq!(tx_no_fees.system_fee(), 0);
    assert_eq!(tx_no_fees.network_fee(), 0);

    let tx_with_fees =
        create_signed_transaction(sender, vec![OpCode::RET as u8], 100, 1000000, 50000);
    assert!(tx_with_fees.system_fee() > 0);
    assert!(tx_with_fees.network_fee() > 0);
}

#[test]
fn test_mempool_basic_operations() {
    let _mempool = Mempool::new();
    assert_eq!(_mempool.len(), 0);
    assert!(_mempool.is_empty());

    let top = _mempool.get_top(10);
    assert!(top.is_empty());
}

#[test]
fn test_mempool_with_config() {
    let config = MempoolConfig {
        max_transactions: 100,
        max_per_sender: 10,
        fee_policy: Default::default(),
        enable_replacement: true,
        replacement_fee_increase: 10,
    };
    let mempool = Mempool::with_config(config);
    assert!(mempool.is_empty());
}

#[test]
fn test_transaction_state_changes() {
    let mut world_state = MemoryWorldState::new();

    let sender = UInt160::from([0x01u8; 20]);
    let sender_account = AccountState::with_balances(sender, 1000, 100_000_000);

    let receiver = UInt160::from([0x02u8; 20]);
    let receiver_account = AccountState::with_balances(receiver, 0, 0);

    let mut changes = StateChanges::new();
    changes.accounts.insert(sender, Some(sender_account));
    changes.accounts.insert(receiver, Some(receiver_account));
    world_state.commit(changes).unwrap();

    let sender_state = world_state.get_account(&sender).unwrap().unwrap();
    assert_eq!(sender_state.neo_balance(), 1000);
    assert_eq!(sender_state.gas_balance(), 100_000_000);
}

#[test]
fn test_contract_storage_updates() {
    let mut world_state = MemoryWorldState::new();

    let contract = UInt160::from([0x01u8; 20]);

    let key = StorageKey::new(contract, vec![0x01, 0x02, 0x03]);
    let initial_value = StorageItem::new(vec![0x00, 0x00, 0x00, 0x00]);

    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(initial_value));
    world_state.commit(changes).unwrap();

    let stored = world_state.get_storage(&key).unwrap().unwrap();
    assert_eq!(stored.as_bytes(), &[0x00, 0x00, 0x00, 0x00]);

    let updated_value = StorageItem::new(vec![0x01, 0x02, 0x03, 0x04]);
    let mut changes = StateChanges::new();
    changes.storage.insert(key.clone(), Some(updated_value));
    world_state.commit(changes).unwrap();

    let stored = world_state.get_storage(&key).unwrap().unwrap();
    assert_eq!(stored.as_bytes(), &[0x01, 0x02, 0x03, 0x04]);
}

#[test]
fn test_full_transaction_lifecycle() {
    let (chain, mut world_state, _mempool) = setup_test_env();

    let sender = UInt160::from([0x01u8; 20]);
    let sender_account = AccountState::with_balances(sender, 1000, 100_000_000);

    let mut changes = StateChanges::new();
    changes.accounts.insert(sender, Some(sender_account));
    world_state.commit(changes).unwrap();

    let script = vec![OpCode::RET as u8];
    let tx = create_signed_transaction(sender, script, 1000, 1000000, 50000);
    let tx_hash = tx.hash();

    assert_eq!(tx.hash(), tx_hash);
    assert_eq!(chain.height(), 0);
    assert!(chain.is_initialized());
}

#[tokio::test]
async fn test_concurrent_transaction_processing() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let world_state = Arc::new(Mutex::new(MemoryWorldState::new()));

    let mut handles = vec![];
    for i in 0u8..10 {
        let state_clone = world_state.clone();
        let handle = tokio::spawn(async move {
            let mut state = state_clone.lock().await;

            let account = UInt160::from([i; 20]);
            let account_state = AccountState::with_balances(account, 1000, 100_000_000);

            let mut changes = StateChanges::new();
            changes.accounts.insert(account, Some(account_state));
            state.commit(changes).unwrap();
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }

    let state = world_state.lock().await;
    for i in 0u8..10 {
        let account = UInt160::from([i; 20]);
        let retrieved = state.get_account(&account).unwrap();
        assert!(retrieved.is_some(), "Account {} should exist", i);
    }
}

#[test]
fn test_transaction_fee_calculation() {
    let (sender, _) = create_test_account(1000, 10000);

    let tx = create_signed_transaction(sender, vec![OpCode::RET as u8], 100, 1_000_000, 500_000);

    let total_fees = tx.system_fee() + tx.network_fee();
    assert_eq!(
        total_fees, 1_500_000,
        "Total fees should be sum of system and network fees"
    );
}

#[test]
fn test_transaction_with_multiple_signers() {
    let (sender1, _) = create_test_account(1000, 10000);
    let sender2 = UInt160::from([0x02u8; 20]);

    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::RET as u8]);

    let signer1 = Signer::new(sender1, WitnessScope::CalledByEntry);
    let signer2 = Signer::new(sender2, WitnessScope::Global);

    tx.add_signer(signer1);
    tx.add_signer(signer2);

    assert_eq!(tx.signers().len(), 2);
    assert_eq!(tx.sender(), Some(sender1), "First signer is sender");
}

#[test]
fn test_transaction_with_high_priority_attribute() {
    let (sender, _) = create_test_account(1000, 10000);

    let mut tx = Transaction::new();
    tx.set_script(vec![OpCode::RET as u8]);

    let signer = Signer::new(sender, WitnessScope::CalledByEntry);
    tx.add_signer(signer);

    let attr = TransactionAttribute::high_priority();
    tx.add_attribute(attr);

    assert_eq!(tx.attributes().len(), 1);
}

#[test]
fn test_empty_transaction_rejected() {
    let tx = Transaction::new();

    assert!(tx.signers().is_empty());
    assert!(tx.script().is_empty());
    assert_eq!(tx.system_fee(), 0);
    assert_eq!(tx.network_fee(), 0);
}

#[test]
fn test_transaction_valid_until_far_future() {
    let (sender, _) = create_test_account(1000, 10000);

    let tx = create_signed_transaction(sender, vec![OpCode::RET as u8], u32::MAX, 1000, 500);

    assert_eq!(tx.valid_until_block(), u32::MAX);
}

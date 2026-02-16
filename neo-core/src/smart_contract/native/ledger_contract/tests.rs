use super::*;
use crate::network::p2p::payloads::Transaction;
use crate::network::p2p::payloads::signer::Signer;
use crate::network::p2p::payloads::witness::Witness;
use crate::{UInt160, WitnessScope};

fn make_signed_transaction() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_valid_until_block(10);
    tx.add_signer(Signer::new(
        UInt160::default(),
        WitnessScope::CALLED_BY_ENTRY,
    ));
    tx.add_witness(Witness::new());
    tx
}

#[test]
fn update_vm_state_overwrites_persisted_value() {
    let ledger = LedgerContract::new();
    let snapshot = DataCache::new(false);

    let mut tx = make_signed_transaction();
    tx.set_script(vec![0xAA]);
    let state = PersistedTransactionState::new(&tx, 42);
    ledger
        .persist_transaction_state(&snapshot, &state)
        .expect("persist state");

    let hash = tx.hash();
    ledger
        .update_transaction_vm_state(&snapshot, &hash, VMState::HALT)
        .expect("update state");

    let stored = ledger
        .get_transaction_state(&snapshot, &hash)
        .expect("read state")
        .expect("state present");
    assert_eq!(stored.vm_state(), VMState::HALT);
    assert_eq!(stored.block_index(), 42);
}

#[test]
fn batch_vm_state_update_applies_all_entries() {
    let ledger = LedgerContract::new();
    let snapshot = DataCache::new(false);

    let mut tx1 = make_signed_transaction();
    tx1.set_nonce(100);
    tx1.set_script(vec![0x01]);
    let mut tx2 = make_signed_transaction();
    tx2.set_nonce(200);
    tx2.set_script(vec![0x02]);

    let state1 = PersistedTransactionState::new(&tx1, 1);
    let state2 = PersistedTransactionState::new(&tx2, 2);
    ledger
        .persist_transaction_state(&snapshot, &state1)
        .expect("state1");
    ledger
        .persist_transaction_state(&snapshot, &state2)
        .expect("state2");

    let updates = vec![(tx1.hash(), VMState::FAULT), (tx2.hash(), VMState::HALT)];
    ledger
        .update_transaction_vm_states(&snapshot, &updates)
        .expect("updates");

    let state1 = ledger
        .get_transaction_state(&snapshot, &updates[0].0)
        .unwrap()
        .unwrap();
    let state2 = ledger
        .get_transaction_state(&snapshot, &updates[1].0)
        .unwrap()
        .unwrap();

    assert_eq!(state1.vm_state(), VMState::FAULT);
    assert_eq!(state2.vm_state(), VMState::HALT);
}

#[test]
fn ledger_transaction_states_mark_vm_state() {
    let mut tx = Transaction::new();
    tx.set_script(vec![0x10]);
    let hash = tx.hash();
    let mut states = LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, 0)]);
    let updated = states.mark_vm_state(&hash, VMState::FAULT);
    assert!(updated);
    let updates = states.into_updates();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].1, VMState::FAULT);
}

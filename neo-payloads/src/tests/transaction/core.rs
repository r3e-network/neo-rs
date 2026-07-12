use super::*;
use crate::{Block, Header, VerifiableContainer};
use neo_primitives::WitnessScope;
use neo_vm_rs::OpCode;
use std::sync::Arc;

fn transaction_with_script(script: Vec<u8>) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_script(script);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

#[test]
fn transaction_is_its_own_verifiable_container() {
    use crate::VerifiableExt;
    let tx = transaction_with_script(vec![OpCode::NOP.byte()]);
    // Witness verification must install the real Transaction (with its
    // signers/scopes) as the engine's script container — C# Helper.VerifyWitness
    // passes the IVerifiable itself. Without this override the default returns
    // None, the engine falls back to a hash-only wrapper, and CheckWitness can't
    // see signers during verification.
    let as_tx = tx
        .as_transaction()
        .expect("Transaction::as_transaction must return Some (C# parity)");
    assert!(
        std::ptr::eq(as_tx, &tx),
        "must return the transaction itself"
    );
    assert_eq!(as_tx.signers().len(), 1);
    assert_eq!(tx.witnesses().len(), 1);
    assert_eq!(
        crate::VerifiableExt::witnesses(&tx).len(),
        1,
        "verification helpers must see Transaction.Witnesses like C#"
    );
}

#[test]
fn block_backed_transaction_container_borrows_payload_without_cloning() {
    let transaction = transaction_with_script(vec![OpCode::RET.byte()]);
    let block = Arc::new(Block::from_parts(Header::new(), vec![transaction]));
    let expected = &block.transactions[0];
    let container = VerifiableContainer::transaction_in_block(Arc::clone(&block), 0)
        .expect("transaction at block position zero");
    let actual = container
        .as_transaction()
        .expect("block-backed container is a transaction");

    assert!(std::ptr::eq(actual, expected));
    assert_eq!(
        neo_primitives::Verifiable::hash(&container).expect("container hash"),
        expected.try_hash().expect("transaction hash")
    );
    assert!(VerifiableContainer::transaction_in_block(block, 1).is_none());
}

#[test]
fn serializable_payload_hash_is_single_sha256_of_unsigned_transaction() {
    let tx = transaction_with_script(vec![OpCode::RET.byte()]);
    let unsigned = tx.try_get_hash_data().expect("unsigned transaction");
    let first_digest = Crypto::sha256(&unsigned);
    let second_digest = Crypto::sha256(&first_digest);
    let expected_single = UInt256::from(first_digest);
    let unexpected_double = UInt256::from(second_digest);

    assert_eq!(
        <Transaction as neo_primitives::SerializablePayload>::hash(&tx),
        expected_single
    );
    assert_eq!(tx.try_hash().expect("transaction hash"), expected_single);
    assert_ne!(
        <Transaction as neo_primitives::SerializablePayload>::hash(&tx),
        unexpected_double
    );
}

#[test]
fn cloned_transaction_preserves_cached_hash_and_size() {
    let tx = transaction_with_script(vec![OpCode::RET.byte()]);
    let expected_hash = tx.try_hash().expect("transaction hash");
    let expected_size = <Transaction as Serializable>::size(&tx);

    let mut cloned = tx.clone();

    assert_eq!(*cloned._hash.lock(), Some(expected_hash));
    assert_eq!(*cloned._size.lock(), Some(expected_size));

    cloned.set_script(vec![OpCode::NOP.byte()]);
    assert_eq!(*cloned._hash.lock(), None);
    assert_eq!(*cloned._size.lock(), None);
    assert_eq!(*tx._hash.lock(), Some(expected_hash));
    assert_eq!(*tx._size.lock(), Some(expected_size));
}

#[test]
fn verifiable_hash_uses_transaction_hash_cache() {
    let tx = transaction_with_script(vec![OpCode::RET.byte()]);
    assert_eq!(*tx._hash.lock(), None);

    let expected_hash = <Transaction as neo_primitives::Verifiable>::hash(&tx)
        .expect("verifiable transaction hash");

    assert_eq!(*tx._hash.lock(), Some(expected_hash));
    assert_eq!(tx.try_hash().expect("transaction hash"), expected_hash);
}

#[test]
fn try_get_hash_data_rejects_oversized_script() {
    let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

    assert!(tx.try_get_hash_data().is_err());
}

#[test]
fn try_to_bytes_rejects_oversized_script() {
    let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

    assert!(tx.try_to_bytes().is_err());
}

#[test]
fn try_hash_rejects_oversized_script_without_caching_zero_hash() {
    let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

    assert!(tx.try_hash().is_err());
    assert!(!matches!(*tx._hash.lock(), Some(hash) if hash == UInt256::zero()));
}

#[test]
fn serializable_payload_hash_fails_closed_for_oversized_script() {
    let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
    let trait_hash = <Transaction as neo_primitives::SerializablePayload>::hash(&tx);

    assert_eq!(trait_hash, UInt256::zero());
    assert!(
        !matches!(*tx._hash.lock(), Some(hash) if hash == UInt256::zero()),
        "invalid transactions must not cache a synthetic zero hash"
    );
}

#[test]
fn verifiable_hash_rejects_oversized_script() {
    let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);

    assert!(<Transaction as neo_primitives::Verifiable>::hash(&tx).is_err());
}

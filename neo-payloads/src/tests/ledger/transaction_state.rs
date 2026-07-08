use super::TransactionState;
use crate::Witness;
use crate::{signer::Signer, transaction::Transaction};
use neo_primitives::UInt160;
use neo_primitives::WitnessScope;
use neo_serialization::BinarySerializer;
use neo_vm::Interoperable;
use neo_vm_rs::{ExecutionEngineLimits, OpCode, StackValue, VmState as VMState};

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(x), Buffer(y)) => x == y,
        (Array(x), Array(y)) | (Struct(x), Struct(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

fn sample_transaction(nonce: u32, network_fee: i64) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(network_fee);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

fn stack_bytes(state: &TransactionState) -> Vec<u8> {
    BinarySerializer::serialize_stack_value(&stack_value(state), &ExecutionEngineLimits::default())
        .expect("serialize stack item")
}

fn stack_value(state: &TransactionState) -> StackValue {
    state.try_to_stack_value().unwrap()
}

fn decode_stack_value(bytes: &[u8]) -> StackValue {
    BinarySerializer::deserialize_stack_value(bytes).expect("deserialize stack value")
}

#[test]
fn transaction_state_projects_conflict_stub_to_neo_vm_rs_stack_value() {
    let state = TransactionState::new(7, None, VMState::NONE);

    let left = stack_value(&state);
    let right = StackValue::Struct(vec![StackValue::Integer(7)]);
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn transaction_state_reads_from_neo_vm_rs_stack_value() {
    let tx = sample_transaction(99, 200);
    let tx_bytes = TransactionState::serialize_transaction(&tx).unwrap();
    let mut state = TransactionState::new(0, None, VMState::NONE);

    state
        .from_stack_value(StackValue::Struct(vec![
            StackValue::Integer(11),
            StackValue::ByteString(tx_bytes),
            StackValue::Integer(VMState::HALT.to_byte() as i64),
        ]))
        .unwrap();

    assert_eq!(state.block_index, 11);
    assert_eq!(state.state, VMState::HALT);
    assert_eq!(state.transaction.as_ref().unwrap().hash(), tx.hash());
}

#[test]
fn conflict_stub_roundtrip() {
    let state = TransactionState::new(7, None, VMState::NONE);
    let value = stack_value(&state);

    let mut parsed = TransactionState::new(0, None, VMState::HALT);
    parsed.from_stack_value(value).unwrap();

    assert_eq!(parsed.block_index, 7);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

#[test]
fn full_roundtrip_preserves_transaction_and_state() {
    let mut tx = sample_transaction(7, 0);
    tx.set_script(vec![0x01, 0x02, 0x03]);

    let state = TransactionState::new(42, Some(tx.clone()), VMState::HALT);
    let value = stack_value(&state);

    let mut parsed = TransactionState::new(0, None, VMState::NONE);
    parsed.from_stack_value(value).unwrap();

    assert_eq!(parsed.block_index, 42);
    assert_eq!(parsed.state, VMState::HALT);
    assert!(parsed.transaction.is_some());
    assert_eq!(parsed.transaction.unwrap().hash(), tx.hash());
}

#[test]
fn binary_roundtrip_matches_csharp_transaction_state() {
    let state = TransactionState::new(1, Some(sample_transaction(7, 100)), VMState::NONE);
    let bytes = stack_bytes(&state);

    let mut parsed = TransactionState::new(0, None, VMState::HALT);
    parsed.from_stack_value(decode_stack_value(&bytes)).unwrap();

    assert_eq!(parsed.block_index, 1);
    assert_eq!(
        parsed.transaction.as_ref().unwrap().hash(),
        state.transaction.as_ref().unwrap().hash()
    );
}

#[test]
fn interoperable_projection_matches_stack_value_projection() {
    let state = TransactionState::new(12, Some(sample_transaction(7, 100)), VMState::HALT);
    let expected = stack_value(&state);

    let interop = Interoperable::to_stack_value(&state).unwrap();
    assert!(
        stack_value_struct_eq(&interop, &expected),
        "structural StackValue mismatch: {interop:?} vs {expected:?}"
    );

    let mut parsed = TransactionState::new(0, None, VMState::NONE);
    Interoperable::from_stack_value(&mut parsed, expected).unwrap();

    assert_eq!(parsed.block_index, 12);
    assert_eq!(parsed.state, VMState::HALT);
    assert_eq!(
        parsed.transaction.as_ref().unwrap().hash(),
        state.transaction.as_ref().unwrap().hash()
    );
}

#[test]
fn interoperable_projection_accepts_conflict_stub() {
    let state = TransactionState::new(9, None, VMState::NONE);
    let stack_value = Interoperable::to_stack_value(&state).unwrap();

    let mut parsed = TransactionState::new(0, Some(sample_transaction(1, 0)), VMState::HALT);
    Interoperable::from_stack_value(&mut parsed, stack_value).unwrap();

    assert_eq!(parsed.block_index, 9);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

#[test]
fn transaction_state_rejects_invalid_stack_shapes() {
    let mut parsed = TransactionState::new(0, None, VMState::NONE);

    assert!(parsed.from_stack_value(StackValue::Array(vec![])).is_err());
    assert!(parsed.from_stack_value(StackValue::Struct(vec![])).is_err());
    assert!(
        parsed
            .from_stack_value(StackValue::Struct(vec![
                StackValue::Integer(7),
                StackValue::ByteString(vec![])
            ]))
            .is_err()
    );
}

#[test]
fn transaction_state_rejects_malformed_transaction_bytes() {
    let mut parsed = TransactionState::new(0, None, VMState::NONE);

    let error = parsed
        .from_stack_value(StackValue::Struct(vec![
            StackValue::Integer(7),
            StackValue::ByteString(vec![0xff]),
            StackValue::Integer(VMState::HALT.to_byte() as i64),
        ]))
        .unwrap_err();

    assert!(
        error.to_string().contains("TransactionState transaction"),
        "{error}"
    );
}

#[test]
fn transaction_state_clone_is_independent() {
    let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
    let mut clone = origin.clone();

    assert_eq!(stack_bytes(&origin), stack_bytes(&clone));

    clone
        .transaction
        .as_mut()
        .expect("clone transaction")
        .set_nonce(2);
    assert_ne!(stack_bytes(&origin), stack_bytes(&clone));
}

#[test]
fn transaction_state_from_stack_value_updates_fields() {
    let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
    let mut replica = TransactionState::new(0, None, VMState::HALT);
    replica.from_stack_value(stack_value(&origin)).unwrap();

    assert_eq!(stack_bytes(&replica), stack_bytes(&origin));
    assert_eq!(
        replica.transaction.as_ref().unwrap().nonce(),
        origin.transaction.as_ref().unwrap().nonce()
    );

    let new_origin = TransactionState::new(2, Some(sample_transaction(99, 200)), VMState::NONE);
    replica.from_stack_value(stack_value(&new_origin)).unwrap();

    assert_eq!(stack_bytes(&replica), stack_bytes(&new_origin));
    assert_eq!(replica.block_index, 2);
    assert_eq!(
        replica.transaction.as_ref().unwrap().network_fee(),
        new_origin.transaction.as_ref().unwrap().network_fee()
    );
}

#[test]
fn trimmed_transaction_state_roundtrip_via_binary_serializer() {
    let state = TransactionState::new(7, None, VMState::NONE);
    let bytes = stack_bytes(&state);

    let mut parsed = TransactionState::new(0, Some(sample_transaction(1, 0)), VMState::HALT);
    parsed.from_stack_value(decode_stack_value(&bytes)).unwrap();

    assert_eq!(parsed.block_index, 7);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

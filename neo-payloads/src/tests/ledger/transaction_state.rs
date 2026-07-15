use super::TransactionState;
use crate::Witness;
use crate::{signer::Signer, transaction::Transaction};
use neo_primitives::UInt160;
use neo_primitives::WitnessScope;
use neo_serialization::BinarySerializer;
use neo_vm::Interoperable;
use neo_vm::{ExecutionEngineLimits, OpCode, StackItem, VmState as VMState};

/// Structural equality for stack items. Collection identity is not part of
/// serialized stack data, so structural equality is the correct notion for
/// round-trip and shape assertions.
fn stack_item_struct_eq(a: &StackItem, b: &StackItem) -> bool {
    a.equals(b).unwrap_or(false)
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
    BinarySerializer::serialize(&stack_item(state), &ExecutionEngineLimits::default())
        .expect("serialize stack item")
}

fn stack_item(state: &TransactionState) -> StackItem {
    state.try_to_stack_item().unwrap()
}

fn decode_stack_item(bytes: &[u8]) -> StackItem {
    BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
        .expect("deserialize stack item")
}

#[test]
fn transaction_state_projects_conflict_stub_to_stack_item() {
    let state = TransactionState::new(7, None, VMState::NONE);

    let left = stack_item(&state);
    let right = StackItem::from_struct(vec![StackItem::from_i64(7)]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn transaction_state_reads_from_stack_item() {
    let tx = sample_transaction(99, 200);
    let tx_bytes = TransactionState::serialize_transaction(&tx).unwrap();
    let mut state = TransactionState::new(0, None, VMState::NONE);

    state
        .from_stack_item(StackItem::from_struct(vec![
            StackItem::from_i64(11),
            StackItem::from_byte_string(tx_bytes),
            StackItem::from_i64(i64::from(VMState::HALT.to_byte())),
        ]))
        .unwrap();

    assert_eq!(state.block_index, 11);
    assert_eq!(state.state, VMState::HALT);
    assert_eq!(state.transaction.as_ref().unwrap().hash(), tx.hash());
}

#[test]
fn conflict_stub_roundtrip() {
    let state = TransactionState::new(7, None, VMState::NONE);
    let value = stack_item(&state);

    let mut parsed = TransactionState::new(0, None, VMState::HALT);
    parsed.from_stack_item(value).unwrap();

    assert_eq!(parsed.block_index, 7);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

#[test]
fn full_roundtrip_preserves_transaction_and_state() {
    let mut tx = sample_transaction(7, 0);
    tx.set_script(vec![0x01, 0x02, 0x03]);

    let state = TransactionState::new(42, Some(tx.clone()), VMState::HALT);
    let value = stack_item(&state);

    let mut parsed = TransactionState::new(0, None, VMState::NONE);
    parsed.from_stack_item(value).unwrap();

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
    parsed.from_stack_item(decode_stack_item(&bytes)).unwrap();

    assert_eq!(parsed.block_index, 1);
    assert_eq!(
        parsed.transaction.as_ref().unwrap().hash(),
        state.transaction.as_ref().unwrap().hash()
    );
}

#[test]
fn interoperable_projection_matches_stack_item_projection() {
    let state = TransactionState::new(12, Some(sample_transaction(7, 100)), VMState::HALT);
    let expected = stack_item(&state);

    let interop = Interoperable::to_stack_item(&state).unwrap();
    assert!(
        stack_item_struct_eq(&interop, &expected),
        "structural StackItem mismatch: {interop:?} vs {expected:?}"
    );

    let mut parsed = TransactionState::new(0, None, VMState::NONE);
    Interoperable::from_stack_item(&mut parsed, expected).unwrap();

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
    let stack_item = Interoperable::to_stack_item(&state).unwrap();

    let mut parsed = TransactionState::new(0, Some(sample_transaction(1, 0)), VMState::HALT);
    Interoperable::from_stack_item(&mut parsed, stack_item).unwrap();

    assert_eq!(parsed.block_index, 9);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

#[test]
fn transaction_state_rejects_invalid_stack_shapes() {
    let mut parsed = TransactionState::new(0, None, VMState::NONE);

    assert!(
        parsed
            .from_stack_item(StackItem::from_array(vec![]))
            .is_err()
    );
    assert!(
        parsed
            .from_stack_item(StackItem::from_struct(vec![]))
            .is_err()
    );
    assert!(
        parsed
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::from_i64(7),
                StackItem::from_byte_string(vec![]),
            ]))
            .is_err()
    );
}

#[test]
fn transaction_state_rejects_malformed_transaction_bytes() {
    let mut parsed = TransactionState::new(0, None, VMState::NONE);

    let error = parsed
        .from_stack_item(StackItem::from_struct(vec![
            StackItem::from_i64(7),
            StackItem::from_byte_string(vec![0xff]),
            StackItem::from_i64(i64::from(VMState::HALT.to_byte())),
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
fn transaction_state_from_stack_item_updates_fields() {
    let origin = TransactionState::new(1, Some(sample_transaction(1, 100)), VMState::NONE);
    let mut replica = TransactionState::new(0, None, VMState::HALT);
    replica.from_stack_item(stack_item(&origin)).unwrap();

    assert_eq!(stack_bytes(&replica), stack_bytes(&origin));
    assert_eq!(
        replica.transaction.as_ref().unwrap().nonce(),
        origin.transaction.as_ref().unwrap().nonce()
    );

    let new_origin = TransactionState::new(2, Some(sample_transaction(99, 200)), VMState::NONE);
    replica.from_stack_item(stack_item(&new_origin)).unwrap();

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
    parsed.from_stack_item(decode_stack_item(&bytes)).unwrap();

    assert_eq!(parsed.block_index, 7);
    assert!(parsed.transaction.is_none());
    assert_eq!(parsed.state, VMState::NONE);
}

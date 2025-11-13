use neo_base::hash::Hash160;
use neo_store::{ColumnId, MemoryStore};
use neo_vm::{VmError, VmValue};

use crate::{
    nef::CallFlags,
    runtime::{context::tests::helpers::new_context, ExecutionContext},
};

fn ctx_with_flags(store: &mut MemoryStore, flags: CallFlags) -> ExecutionContext<'_> {
    let mut ctx = new_context(store);
    ctx.set_call_flags(flags);
    ctx
}

fn fixed_hash(byte: u8) -> Hash160 {
    Hash160::from_slice(&[byte; 20]).expect("hash")
}

#[test]
fn storage_read_requires_read_states() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    store
        .put(
            ColumnId::new("contract"),
            b"key".to_vec(),
            b"value".to_vec(),
        )
        .unwrap();
    let ctx = ctx_with_flags(&mut store, CallFlags::WRITE_STATES);
    let err = ctx.load(ColumnId::new("contract"), b"key").unwrap_err();
    assert!(matches!(
        err,
        crate::error::ContractError::MissingCallFlags(flag) if flag == CallFlags::READ_STATES
    ));
}

#[test]
fn storage_write_requires_write_states() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let mut ctx = ctx_with_flags(&mut store, CallFlags::READ_STATES);
    let err = ctx
        .put(
            ColumnId::new("contract"),
            b"key".to_vec(),
            b"value".to_vec(),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        crate::error::ContractError::MissingCallFlags(flag) if flag == CallFlags::WRITE_STATES
    ));
}

#[test]
fn contract_call_rejects_invalid_flag_bits() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(&fixed_hash(0x01), "foo", 0x80, Vec::new())
        .unwrap_err();
    assert_eq!(err, VmError::InvalidType);
}

#[test]
fn contract_call_requires_subset_of_current_flags() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::READ_STATES);
    let err = ctx
        .handle_contract_call(
            &fixed_hash(0x02),
            "foo",
            CallFlags::WRITE_STATES.bits(),
            Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(err, VmError::NativeFailure(msg) if msg == "insufficient call flags"));
}

#[test]
fn contract_call_reports_not_supported_when_valid() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(
            &fixed_hash(0x03),
            "foo",
            CallFlags::READ_STATES.bits(),
            vec![VmValue::Int(1)],
        )
        .unwrap_err();
    assert!(
        matches!(err, VmError::NativeFailure(msg) if msg == "contract calls are not supported yet")
    );
}

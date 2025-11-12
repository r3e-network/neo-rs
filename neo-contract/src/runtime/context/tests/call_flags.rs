use neo_store::{ColumnId, MemoryStore};

use crate::{
    nef::CallFlags,
    runtime::{context::tests::helpers::new_context, ExecutionContext},
};

fn ctx_with_flags(store: &mut MemoryStore, flags: CallFlags) -> ExecutionContext<'_> {
    let mut ctx = new_context(store);
    ctx.set_call_flags(flags);
    ctx
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

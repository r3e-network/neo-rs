use neo_base::hash::Hash160;
use neo_core::tx::WitnessScope;
use neo_store::{ColumnId, MemoryStore};

use crate::runtime::ExecutionContext;

use super::helpers::{fixed_hash160, signer_with_scope, to_h160};

#[test]
fn legacy_signer_still_passes_check_witness() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let signer = fixed_hash160(0x42);
    let ctx = ExecutionContext::new(&mut store, 1_000, Some(signer));
    assert!(ctx.check_witness(&signer));
}

#[test]
fn called_by_entry_scope_requires_matching_context() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));

    let signer_hash = fixed_hash160(0x01);
    let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
    ctx.set_signers(vec![signer_with_scope(
        signer_hash,
        WitnessScope::CalledByEntry,
    )]);
    ctx.set_entry_script_hash(signer_hash);
    ctx.set_current_script_hash(signer_hash);

    assert!(ctx.check_witness(&signer_hash));

    ctx.set_current_script_hash(fixed_hash160(0xAA));
    assert!(!ctx.check_witness(&signer_hash));
}

#[test]
fn global_scope_succeeds_without_context() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));

    let signer_hash = fixed_hash160(0x22);
    let mut scopes = neo_core::tx::WitnessScopes::new();
    scopes.add_scope(WitnessScope::Global);
    let signer = neo_core::tx::Signer {
        account: to_h160(&signer_hash),
        scopes,
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };

    let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
    ctx.set_signers(vec![signer]);

    assert!(ctx.check_witness(&signer_hash));
}

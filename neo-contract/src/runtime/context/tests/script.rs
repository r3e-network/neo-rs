use neo_base::hash::{hash160, Hash160};
use neo_base::Bytes;
use neo_core::script::Script;
use neo_core::tx::{Signer, WitnessScope, WitnessScopes};
use neo_store::{ColumnId, MemoryStore};

use crate::runtime::ExecutionContext;

use super::helpers::{fixed_hash160, signer_with_scope, to_h160};

#[test]
fn set_script_updates_hashes() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
    let script = Bytes::from(vec![0x01, 0x02, 0x03]);
    let expected = Hash160::from_slice(hash160::<&[u8]>(script.as_ref()).as_ref()).unwrap();
    ctx.set_script(script.clone());
    assert_eq!(ctx.current_script_hash(), Some(expected));
    assert_eq!(ctx.entry_script_hash(), Some(expected));

    let next_script = Bytes::from(vec![0xAA, 0xBB]);
    let next_expected =
        Hash160::from_slice(hash160::<&[u8]>(next_script.as_ref()).as_ref()).unwrap();
    ctx.set_script(next_script);
    assert_eq!(ctx.current_script_hash(), Some(next_expected));
    assert_eq!(ctx.entry_script_hash(), Some(expected));
}

#[test]
fn load_transaction_context_populates_signers_and_script() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let signer_hash = fixed_hash160(0x11);
    let signer = Signer {
        account: to_h160(&signer_hash),
        scopes: WitnessScopes::from(WitnessScope::CalledByEntry),
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };
    let tx = neo_core::tx::Transaction {
        version: 0,
        nonce: 0,
        sender: Some(to_h160(&signer_hash)),
        system_fee: 0,
        network_fee: 0,
        valid_until_block: 100,
        attributes: Vec::new(),
        signers: vec![signer.clone()],
        script: Script::new(vec![0xAA, 0xBB]),
        witnesses: Vec::new(),
    };
    let mut ctx = ExecutionContext::new(&mut store, 1_000, None);
    ctx.load_transaction_context(&tx);
    assert_eq!(ctx.signers().len(), 1);
    assert_eq!(ctx.signers()[0].account, signer.account);
    let expected = Hash160::from_slice(hash160::<&[u8]>(tx.script.as_bytes()).as_ref()).unwrap();
    assert_eq!(ctx.current_script_hash(), Some(expected));
    assert_eq!(ctx.entry_script_hash(), Some(expected));
    assert!(ctx.check_witness(&signer_hash));
}

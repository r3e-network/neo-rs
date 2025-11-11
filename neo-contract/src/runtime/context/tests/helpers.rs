use neo_base::hash::Hash160;
use neo_core::tx::{Signer, WitnessScope, WitnessScopes};

use crate::runtime::ExecutionContext;

pub(crate) fn fixed_hash160(byte: u8) -> Hash160 {
    Hash160::from_slice(&[byte; 20]).expect("20 bytes")
}

pub(crate) fn to_h160(hash: &Hash160) -> neo_core::h160::H160 {
    let mut buf = [0u8; 20];
    buf.copy_from_slice(hash.as_slice());
    neo_core::h160::H160::from_le_bytes(buf)
}

pub(crate) fn signer_with_scope(account: Hash160, scope: WitnessScope) -> Signer {
    let mut scopes = WitnessScopes::new();
    scopes.add_scope(scope);
    Signer {
        account: to_h160(&account),
        scopes,
        allowed_contract: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    }
}

pub(crate) fn new_context<'a>(store: &'a mut neo_store::MemoryStore) -> ExecutionContext<'a> {
    store.create_column(neo_store::ColumnId::new("contract"));
    ExecutionContext::new(store, 1_000, None)
}

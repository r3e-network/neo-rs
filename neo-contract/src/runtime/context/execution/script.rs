use alloc::vec::Vec;

use neo_base::{
    hash::{hash160, Hash160},
    Bytes,
};
use neo_core::{script::Script, tx::Tx};
use neo_crypto::ecc256::PublicKey;

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub fn script(&self) -> &Bytes {
        &self.script
    }

    pub fn set_script(&mut self, script: Bytes) {
        if let Some(hash) = script_hash_from_bytes(&script) {
            self.current_script_hash = Some(hash);
            if self.entry_script_hash.is_none() {
                self.entry_script_hash = Some(hash);
            }
        }
        self.script = script;
    }

    pub fn set_script_from_core(&mut self, script: &Script) {
        self.set_script(Bytes::from(script.as_bytes().to_vec()));
    }

    pub fn set_entry_script_hash(&mut self, hash: Hash160) {
        self.entry_script_hash = Some(hash);
    }

    pub fn entry_script_hash(&self) -> Option<Hash160> {
        self.entry_script_hash
    }

    pub fn set_current_script_hash(&mut self, hash: Hash160) {
        self.current_script_hash = Some(hash);
    }

    pub fn current_script_hash(&self) -> Option<Hash160> {
        self.current_script_hash
    }

    pub fn set_calling_script_hash(&mut self, hash: Option<Hash160>) {
        self.calling_script_hash = hash;
    }

    pub fn calling_script_hash(&self) -> Option<Hash160> {
        self.calling_script_hash
    }

    pub fn set_current_contract_groups(&mut self, groups: Vec<PublicKey>) {
        self.current_contract_groups = groups;
    }

    pub fn current_contract_groups(&self) -> &[PublicKey] {
        &self.current_contract_groups
    }

    pub fn set_calling_contract_groups(&mut self, groups: Vec<PublicKey>) {
        self.calling_contract_groups = groups;
    }

    pub fn calling_contract_groups(&self) -> &[PublicKey] {
        &self.calling_contract_groups
    }

    pub fn load_transaction_context(&mut self, tx: &Tx) {
        self.set_signers_from(&tx.signers);
        self.set_script_from_core(&tx.script);
        self.set_calling_script_hash(None);
        self.set_current_contract_groups(Vec::new());
        self.set_calling_contract_groups(Vec::new());
    }
}

fn script_hash_from_bytes(script: &Bytes) -> Option<Hash160> {
    if script.is_empty() {
        return None;
    }
    let raw = hash160::<&[u8]>(script.as_ref());
    Hash160::from_slice(raw.as_ref()).ok()
}

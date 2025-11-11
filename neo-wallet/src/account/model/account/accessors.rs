use neo_base::hash::Hash160;
use neo_crypto::ecc256::PublicKey;
use serde_json::Value;

use super::Account;
use crate::{account::contract::Contract, signer::SignerScopes};

impl Account {
    pub fn script_hash(&self) -> Hash160 {
        self.script_hash
    }

    pub fn public_key(&self) -> Option<&PublicKey> {
        self.public_key.as_ref()
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = Some(label.into());
    }

    pub fn clear_label(&mut self) {
        self.label = None;
    }

    pub fn is_watch_only(&self) -> bool {
        self.private_key.is_none()
    }

    pub fn is_default(&self) -> bool {
        self.is_default
    }

    pub fn set_default(&mut self, value: bool) {
        self.is_default = value;
    }

    pub fn is_locked(&self) -> bool {
        self.lock
    }

    pub fn set_lock(&mut self, value: bool) {
        self.lock = value;
    }

    pub fn contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }

    pub fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }

    pub fn extra(&self) -> Option<&Value> {
        self.extra.as_ref()
    }

    pub fn set_extra(&mut self, extra: Option<Value>) {
        self.extra = extra;
    }

    pub fn signer_scopes(&self) -> SignerScopes {
        self.signer_scopes
    }

    pub fn set_signer_scopes(&mut self, scopes: SignerScopes) {
        self.signer_scopes = scopes;
    }

    pub fn allowed_contracts(&self) -> &[Hash160] {
        &self.allowed_contracts
    }

    pub fn set_allowed_contracts(&mut self, contracts: Vec<Hash160>) {
        self.allowed_contracts = contracts;
    }

    pub fn allowed_groups(&self) -> &[Vec<u8>] {
        &self.allowed_groups
    }

    pub fn set_allowed_groups(&mut self, groups: Vec<Vec<u8>>) {
        self.allowed_groups = groups;
    }
}

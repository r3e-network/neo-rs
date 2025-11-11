use alloc::vec::Vec;

use neo_base::hash::Hash160;

use super::SignerScopes;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signer {
    account: Hash160,
    scopes: SignerScopes,
    allowed_contracts: Vec<Hash160>,
    allowed_groups: Vec<Vec<u8>>,
}

impl Signer {
    pub fn new(account: Hash160) -> Self {
        Self {
            account,
            scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }

    pub fn with_scopes(mut self, scopes: SignerScopes) -> Self {
        self.scopes = scopes;
        self
    }

    pub fn account(&self) -> Hash160 {
        self.account
    }

    pub fn scopes(&self) -> SignerScopes {
        self.scopes
    }

    pub fn set_scopes(&mut self, scopes: SignerScopes) {
        self.scopes = scopes;
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

    pub fn ensure_called_by_entry(&mut self) {
        if self.scopes.is_empty() {
            self.scopes = SignerScopes::CALLED_BY_ENTRY;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn default_signer_uses_called_by_entry() {
        let hash = Hash160::from_slice(&hex!("17b24dbdc30b30f33d05a281a81f0c0a5f94b8c0")).unwrap();
        let signer = Signer::new(hash);
        assert_eq!(signer.account(), hash);
        assert_eq!(signer.scopes(), SignerScopes::CALLED_BY_ENTRY);
        assert!(signer.allowed_contracts().is_empty());
        assert!(signer.allowed_groups().is_empty());
    }
}

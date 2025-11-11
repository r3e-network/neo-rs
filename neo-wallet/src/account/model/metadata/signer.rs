use super::super::account::Account;
use crate::signer::Signer;

impl Account {
    pub fn to_signer(&self) -> Signer {
        let mut signer = Signer::new(self.script_hash);
        signer.set_scopes(self.signer_scopes);
        signer.set_allowed_contracts(self.allowed_contracts.clone());
        signer.set_allowed_groups(self.allowed_groups.clone());
        signer
    }
}

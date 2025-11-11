use super::*;

#[derive(Default, Clone)]
pub struct Wallet {
    pub(crate) accounts: BTreeMap<Hash160, Account>,
}

impl Wallet {
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.accounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
    }

    pub fn account(&self, hash: &Hash160) -> Option<&Account> {
        self.accounts.get(hash)
    }

    pub fn account_details(&self) -> Vec<AccountDetails> {
        self.accounts
            .values()
            .map(|account| AccountDetails {
                script_hash: account.script_hash(),
                label: account.label().map(|v| v.to_string()),
                is_default: account.is_default(),
                lock: account.is_locked(),
                scopes: account.signer_scopes(),
                allowed_contracts: account.allowed_contracts().to_vec(),
                allowed_groups: account.allowed_groups().to_vec(),
            })
            .collect()
    }
}

use super::*;

impl<S: Store + ?Sized> WalletStorage<S> {
    pub fn accounts(&self, password: &str) -> Result<Vec<Account>, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        let mut accounts: Vec<Account> = wallet.accounts.values().cloned().collect();
        for account in &mut accounts {
            if let Some(metadata) = self.signer_metadata.get(&account.script_hash()) {
                account.set_signer_scopes(metadata.scopes());
                account.set_allowed_contracts(metadata.allowed_contracts.clone());
                account.set_allowed_groups(metadata.allowed_groups.clone());
            }
        }
        Ok(accounts)
    }

    pub fn script_hashes(&self) -> Vec<Hash160> {
        self.keystore
            .entries
            .iter()
            .map(|entry| entry.script_hash)
            .collect()
    }

    pub fn account_details(&self, password: &str) -> Result<Vec<AccountDetails>, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        let mut details = wallet.account_details();
        for detail in &mut details {
            if let Some(metadata) = self.signer_metadata.get(&detail.script_hash) {
                detail.scopes = metadata.scopes();
                detail.allowed_contracts = metadata.allowed_contracts.clone();
                detail.allowed_groups = metadata.allowed_groups.clone();
            }
        }
        Ok(details)
    }
}

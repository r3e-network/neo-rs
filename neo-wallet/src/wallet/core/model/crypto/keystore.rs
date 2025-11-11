use crate::{
    account::{self, Account},
    keystore::{decrypt_entry, Keystore},
    wallet::core::model::wallet::Wallet,
    WalletError,
};

impl Wallet {
    pub fn to_keystore(&self, password: &str) -> Result<Keystore, WalletError> {
        let accounts: Vec<_> = self.accounts.values().cloned().collect();
        Keystore::from_accounts(&accounts, password)
    }

    pub fn from_keystore(keystore: &Keystore, password: &str) -> Result<Self, WalletError> {
        let mut wallet = Wallet::new();
        for entry in &keystore.entries {
            let private = decrypt_entry(entry, password)?;
            let account = Account::from_private_key(private)?;
            if account.script_hash() != entry.script_hash {
                return Err(WalletError::IntegrityMismatch);
            }
            wallet.add_account(account)?;
        }
        for watch in &keystore.watch_only {
            let contract = watch
                .contract
                .as_ref()
                .map(account::contract_from_nep6)
                .transpose()?;
            let account = Account::watch_only_from_script(watch.script_hash, contract);
            wallet.add_account(account)?;
        }
        Ok(wallet)
    }
}

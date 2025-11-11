use super::*;

impl Wallet {
    pub fn add_account(&mut self, mut account: Account) -> Result<(), WalletError> {
        let hash = account.script_hash();
        if let Some(existing) = self.accounts.get(&hash) {
            if existing.is_watch_only() && !account.is_watch_only() {
                if let Some(label) = existing.label() {
                    account.set_label(label.to_string());
                }
                account.set_default(existing.is_default() || account.is_default());
                account.set_lock(existing.is_locked());
                self.accounts.remove(&hash);
            } else {
                return Err(WalletError::DuplicateAccount);
            }
        }
        let is_default = account.is_default();
        self.accounts.insert(hash, account);
        if is_default {
            self.set_default_internal(&hash)?;
        }
        Ok(())
    }

    pub fn remove_account(&mut self, hash: &Hash160) -> Result<(), WalletError> {
        self.accounts
            .remove(hash)
            .map(|_| ())
            .ok_or(WalletError::AccountNotFound)
    }

    pub fn mark_default(&mut self, hash: &Hash160) -> Result<(), WalletError> {
        self.set_default_internal(hash)
    }

    pub fn set_lock(&mut self, hash: &Hash160, lock: bool) -> Result<(), WalletError> {
        let account = self
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        account.set_lock(lock);
        Ok(())
    }

    pub fn set_label(&mut self, hash: &Hash160, label: Option<String>) -> Result<(), WalletError> {
        let account = self
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        match label {
            Some(label) => account.set_label(label),
            None => account.clear_label(),
        }
        Ok(())
    }

    pub(crate) fn set_default_internal(&mut self, target: &Hash160) -> Result<(), WalletError> {
        if !self.accounts.contains_key(target) {
            return Err(WalletError::AccountNotFound);
        }
        for (hash, account) in self.accounts.iter_mut() {
            account.set_default(hash == target);
        }
        Ok(())
    }
}

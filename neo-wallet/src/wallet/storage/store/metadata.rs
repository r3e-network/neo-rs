use super::*;

impl<S: Store + ?Sized> WalletStorage<S> {
    pub fn update_signer_metadata(
        &mut self,
        hash: &Hash160,
        password: &str,
        scopes: SignerScopes,
        allowed_contracts: Vec<Hash160>,
        allowed_groups: Vec<Vec<u8>>,
    ) -> Result<(), WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let account = wallet
            .accounts
            .get_mut(hash)
            .ok_or(WalletError::AccountNotFound)?;
        account.update_signer_metadata(scopes, allowed_contracts, allowed_groups)?;
        let metadata = StoredSignerMetadata::from_account(account);
        if metadata.is_default() {
            self.signer_metadata.remove(hash);
        } else {
            self.signer_metadata.insert(*hash, metadata);
        }
        self.store_wallet(wallet, password)
    }
}

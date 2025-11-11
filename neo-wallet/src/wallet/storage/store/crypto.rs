use super::*;

impl<S: Store + ?Sized> WalletStorage<S> {
    pub fn import_private_key(
        &mut self,
        private: PrivateKey,
        password: &str,
    ) -> Result<Account, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let account = Account::from_private_key(private)?;
        let hash = account.script_hash();
        wallet.add_account(account.clone())?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, password)?;
        Ok(account)
    }

    pub fn remove_account(&mut self, hash: &Hash160, password: &str) -> Result<(), WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.remove_account(hash)?;
        self.signer_metadata.remove(hash);
        self.store_wallet(wallet, password)
    }

    pub fn sign(
        &self,
        hash: &Hash160,
        payload: &[u8],
        password: &str,
    ) -> Result<SignatureBytes, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.sign(hash, payload)
    }

    pub fn import_wif(
        &mut self,
        wif: &str,
        password: &str,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, password)?;
        let hash = wallet.import_wif(wif, make_default)?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, password)?;
        Ok(hash)
    }

    pub fn import_nep2(
        &mut self,
        nep2: &str,
        passphrase: &str,
        wallet_password: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut wallet = Wallet::from_keystore(&self.keystore, wallet_password)?;
        let hash = wallet.import_nep2(nep2, passphrase, scrypt, address_version, make_default)?;
        self.signer_metadata.remove(&hash);
        self.store_wallet(wallet, wallet_password)?;
        Ok(hash)
    }

    pub fn export_wif(&self, hash: &Hash160, password: &str) -> Result<String, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, password)?;
        wallet.export_wif(hash)
    }

    pub fn export_nep2(
        &self,
        hash: &Hash160,
        wallet_password: &str,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<String, WalletError> {
        let wallet = Wallet::from_keystore(&self.keystore, wallet_password)?;
        wallet.export_nep2(hash, passphrase, scrypt, address_version)
    }
}

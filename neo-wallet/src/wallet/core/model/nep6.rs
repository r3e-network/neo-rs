use super::*;

impl Wallet {
    pub fn to_nep6_wallet(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
        password: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<Nep6Wallet, WalletError> {
        let mut accounts = Vec::with_capacity(self.accounts.len());
        for account in self.accounts.values() {
            let encrypted_key = match account.signer_key() {
                Some(private) => Some(encrypt_nep2(private, password, address_version, scrypt)?),
                None => None,
            };
            accounts.push(account.to_nep6_account(address_version, encrypted_key)?);
        }

        Ok(Nep6Wallet {
            name: name.into(),
            version: version.into(),
            scrypt: scrypt.into(),
            accounts,
            extra: None,
        })
    }

    pub fn from_nep6_wallet(
        nep6: &Nep6Wallet,
        password: Option<&str>,
        address_version: AddressVersion,
    ) -> Result<Self, WalletError> {
        let mut wallet = Wallet::new();
        for account in &nep6.accounts {
            let imported =
                Account::from_nep6_account(account, address_version, nep6.scrypt, password)?;
            wallet.add_account(imported)?;
        }
        Ok(wallet)
    }
}

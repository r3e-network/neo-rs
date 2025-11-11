use super::*;

impl Account {
    pub fn to_nep6_account(
        &self,
        version: AddressVersion,
        encrypted_key: Option<String>,
    ) -> Result<Nep6Account, WalletError> {
        let contract = self.contract.as_ref().map(contract_to_nep6);
        let extra = embed_signer_extra(&self.extra, self);

        Ok(Nep6Account {
            address: self.script_hash.to_address(version),
            label: self.label.clone(),
            is_default: self.is_default,
            lock: self.lock,
            key: encrypted_key,
            contract,
            extra,
        })
    }

    pub fn from_nep6_account(
        account: &Nep6Account,
        version: AddressVersion,
        scrypt: Nep6Scrypt,
        password: Option<&str>,
    ) -> Result<Self, WalletError> {
        let script_hash = Hash160::from_address(&account.address, version)
            .map_err(|_| WalletError::InvalidAddress(account.address.clone()))?;

        let private_key = match account.key.as_deref() {
            Some(nep2) => {
                let password = password.ok_or(WalletError::PassphraseRequired)?;
                let scrypt_params = scrypt.into();
                Some(neo_crypto::nep2::decrypt_nep2(
                    nep2,
                    password,
                    version,
                    scrypt_params,
                )?)
            }
            None => None,
        };

        let mut public_key = None;
        if let Some(private) = &private_key {
            public_key = Some(
                Keypair::from_private(private.clone())
                    .map_err(|_| WalletError::Crypto("keypair"))?
                    .public_key,
            );
        }

        let contract = if let Some(contract) = &account.contract {
            Some(contract_from_nep6(contract)?)
        } else if let Some(pk) = public_key.as_ref() {
            Some(Contract::signature(pk))
        } else {
            None
        };

        let (clean_extra, scopes, allowed_contracts, allowed_groups) =
            parse_signer_extra(account.extra.clone())?;

        let mut result = Self {
            script_hash,
            public_key,
            private_key,
            label: account.label.clone(),
            is_default: account.is_default,
            lock: account.lock,
            contract,
            extra: clean_extra,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        };

        result.signer_scopes = scopes;
        result.allowed_contracts = allowed_contracts;
        result.allowed_groups = allowed_groups;

        Ok(result)
    }
}

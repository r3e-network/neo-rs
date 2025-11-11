use neo_base::hash::Hash160;
use neo_crypto::ecc256::{Keypair, PrivateKey, PublicKey};

use crate::{account::contract::Contract, signer::SignerScopes, WalletError};

use super::Account;

impl Account {
    pub fn from_private_key(private_key: PrivateKey) -> Result<Self, WalletError> {
        let keypair = Keypair::from_private(private_key.clone())
            .map_err(|_| WalletError::Crypto("keypair"))?;
        let script_hash = keypair.public_key.script_hash();
        Ok(Self {
            script_hash,
            public_key: Some(keypair.public_key.clone()),
            private_key: Some(private_key),
            label: None,
            is_default: false,
            lock: false,
            contract: Some(Contract::signature(&keypair.public_key)),
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        })
    }

    pub fn watch_only(public_key: PublicKey) -> Self {
        let script_hash = public_key.script_hash();
        Self {
            script_hash,
            public_key: Some(public_key.clone()),
            private_key: None,
            label: None,
            is_default: false,
            lock: false,
            contract: Some(Contract::signature(&public_key)),
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }

    pub fn watch_only_from_script(script_hash: Hash160, contract: Option<Contract>) -> Self {
        Self {
            script_hash,
            public_key: None,
            private_key: None,
            label: None,
            is_default: false,
            lock: false,
            contract,
            extra: None,
            signer_scopes: SignerScopes::CALLED_BY_ENTRY,
            allowed_contracts: Vec::new(),
            allowed_groups: Vec::new(),
        }
    }
}

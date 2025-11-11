use neo_crypto::{ecc256::PrivateKey, Secp256r1Sign, SignatureBytes};

use super::super::account::Account;
use crate::WalletError;

impl Account {
    pub fn signer_key(&self) -> Option<&PrivateKey> {
        self.private_key.as_ref()
    }

    pub fn private_key_bytes(&self) -> Option<&[u8]> {
        self.private_key.as_ref().map(|k| k.as_be_bytes())
    }

    pub fn sign(&self, payload: &[u8]) -> Result<SignatureBytes, WalletError> {
        if self.lock {
            return Err(WalletError::AccountLocked);
        }
        let private = self
            .private_key
            .as_ref()
            .ok_or(WalletError::PassphraseRequired)?;
        private
            .secp256r1_sign(payload)
            .map_err(|_| WalletError::Crypto("ecdsa"))
    }
}

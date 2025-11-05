use alloc::string::String;

use neo_base::hash::Hash160;
use neo_crypto::{
    ecc256::{Keypair, PrivateKey, PublicKey},
    Secp256r1Sign, SignatureBytes,
};

use crate::error::WalletError;

#[derive(Clone, Debug)]
pub struct Account {
    script_hash: Hash160,
    public_key: PublicKey,
    private_key: Option<PrivateKey>,
    label: Option<String>,
}

impl Account {
    pub fn from_private_key(private_key: PrivateKey) -> Result<Self, WalletError> {
        let keypair =
            Keypair::from_private(private_key.clone()).map_err(|_| WalletError::Crypto("keypair"))?;
        let script_hash = keypair.public_key.script_hash();
        Ok(Self {
            script_hash,
            public_key: keypair.public_key,
            private_key: Some(private_key),
            label: None,
        })
    }

    pub fn watch_only(public_key: PublicKey) -> Self {
        let script_hash = public_key.script_hash();
        Self {
            script_hash,
            public_key,
            private_key: None,
            label: None,
        }
    }

    pub fn script_hash(&self) -> Hash160 {
        self.script_hash
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = Some(label.into());
    }

    pub fn is_watch_only(&self) -> bool {
        self.private_key.is_none()
    }

    pub fn signer_key(&self) -> Option<&PrivateKey> {
        self.private_key.as_ref()
    }

    pub fn private_key_bytes(&self) -> Option<&[u8]> {
        self.private_key.as_ref().map(|k| k.as_be_bytes())
    }

    pub fn sign(&self, payload: &[u8]) -> Result<SignatureBytes, WalletError> {
        let private = self.private_key.as_ref().ok_or(WalletError::PassphraseRequired)?;
        private
            .secp256r1_sign(payload)
            .map_err(|_| WalletError::Crypto("ecdsa"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn account_from_private_key() {
        let private = PrivateKey::new(hex!(
            "c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75"
        ));
        let account = Account::from_private_key(private.clone()).expect("account");
        assert!(!account.is_watch_only());
        let signature = account.sign(b"neo-wallet").expect("signature");
        assert_eq!(signature.0.len(), 64);
        assert!(account.label().is_none());
    }

    #[test]
    fn watch_only_account() {
        let private = PrivateKey::new([1u8; 32]);
        let account = Account::from_private_key(private.clone()).unwrap();
        let watch = Account::watch_only(account.public_key().clone());
        assert!(watch.is_watch_only());
        assert!(watch.sign(&[1, 2, 3]).is_err());
    }
}

use neo_base::hash::Hash160;
use neo_crypto::SignatureBytes;

use crate::{wallet::core::model::Wallet, WalletError};

impl Wallet {
    pub fn sign(&self, hash: &Hash160, payload: &[u8]) -> Result<SignatureBytes, WalletError> {
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
        account.sign(payload)
    }
}

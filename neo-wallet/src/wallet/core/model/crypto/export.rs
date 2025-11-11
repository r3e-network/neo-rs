use neo_base::{encoding::WifEncode, hash::Hash160, AddressVersion};
use neo_crypto::{nep2::encrypt_nep2, scrypt::ScryptParams};

use crate::{error::WalletError, wallet::core::model::wallet::Wallet};

impl Wallet {
    pub fn export_nep2(
        &self,
        hash: &Hash160,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
    ) -> Result<String, WalletError> {
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
        let private = account.signer_key().ok_or(WalletError::WatchOnly)?;
        encrypt_nep2(private, passphrase, address_version, scrypt).map_err(Into::into)
    }

    pub fn export_wif(&self, hash: &Hash160) -> Result<String, WalletError> {
        let account = self
            .accounts
            .get(hash)
            .ok_or(WalletError::AccountNotFound)?;
        let private = account.signer_key().ok_or(WalletError::WatchOnly)?;
        Ok(private.as_be_bytes().wif_encode(0x80, true))
    }
}

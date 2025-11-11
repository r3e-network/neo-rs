use neo_base::{encoding::WifDecode, hash::Hash160, AddressVersion};
use neo_crypto::{ecc256::PrivateKey, nep2::decrypt_nep2, scrypt::ScryptParams};

use crate::{account::Account, error::WalletError};

use super::super::wallet::Wallet;

impl Wallet {
    pub fn import_private_key(
        &mut self,
        private: PrivateKey,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let mut account = Account::from_private_key(private)?;
        account.set_default(make_default);
        let hash = account.script_hash();
        self.add_account(account)?;
        if make_default {
            self.set_default_internal(&hash)?;
        }
        Ok(hash)
    }

    pub fn import_nep2(
        &mut self,
        nep2: &str,
        passphrase: &str,
        scrypt: ScryptParams,
        address_version: AddressVersion,
        make_default: bool,
    ) -> Result<Hash160, WalletError> {
        let private = decrypt_nep2(nep2, passphrase, address_version, scrypt)?;
        self.import_private_key(private, make_default)
    }

    pub fn import_wif(&mut self, wif: &str, make_default: bool) -> Result<Hash160, WalletError> {
        let decoded = wif
            .wif_decode(33)
            .map_err(|err| WalletError::InvalidWif(err.to_string()))?;
        if decoded.version() != 0x80 {
            return Err(WalletError::InvalidWif("unsupported WIF version".into()));
        }
        let data = decoded.data();
        if data.len() != 32 {
            return Err(WalletError::InvalidWif("invalid private key length".into()));
        }
        let mut buf = [0u8; 32];
        buf.copy_from_slice(data);
        let private = PrivateKey::from_slice(&buf)
            .map_err(|_| WalletError::InvalidWif("invalid private key".into()))?;
        self.import_private_key(private, make_default)
    }
}

use neo_base::Bytes;
use neo_crypto::{
    aes::{Aes256EcbCipher, AES256_KEY_SIZE},
    ecc256::PrivateKey,
    scrypt::{DeriveScryptKey, ScryptParams},
};

use crate::{account::Account, error::WalletError};

use super::KeystoreEntry;

pub fn encrypt_account(
    account: &Account,
    password: &str,
    params: ScryptParams,
    salt: [u8; 16],
) -> Result<KeystoreEntry, WalletError> {
    let private = account
        .signer_key()
        .ok_or(WalletError::PassphraseRequired)?;
    let derived = password
        .derive_scrypt_key::<AES256_KEY_SIZE>(&salt, params)
        .map_err(|_| WalletError::Crypto("scrypt derive"))?;
    let mut plaintext = private.as_be_bytes().to_vec();
    derived
        .as_slice()
        .aes256_ecb_encrypt_aligned(&mut plaintext)
        .map_err(|_| WalletError::Crypto("aes encrypt"))?;
    Ok(KeystoreEntry {
        script_hash: account.script_hash(),
        cipher_text: Bytes::from(plaintext),
        salt,
        params: params.into(),
    })
}

pub fn decrypt_entry(entry: &KeystoreEntry, password: &str) -> Result<PrivateKey, WalletError> {
    let params: ScryptParams = entry.params.into();
    let derived = password
        .derive_scrypt_key::<AES256_KEY_SIZE>(&entry.salt, params)
        .map_err(|_| WalletError::Crypto("scrypt derive"))?;
    let mut buffer = entry.cipher_text.clone().into_vec();
    derived
        .as_slice()
        .aes256_ecb_decrypt_aligned(&mut buffer)
        .map_err(|_| WalletError::Crypto("aes decrypt"))?;
    if buffer.len() != 32 {
        return Err(WalletError::InvalidKeystore);
    }
    let mut array = [0u8; 32];
    array.copy_from_slice(&buffer);
    Ok(PrivateKey::new(array))
}

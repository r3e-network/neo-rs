use alloc::vec::Vec;

use neo_base::{hash::Hash160, Bytes};
use neo_crypto::{
    aes::{Aes256EcbCipher, AES256_KEY_SIZE},
    ecc256::PrivateKey,
    scrypt::{DeriveScryptKey, ScryptParams},
};
#[cfg(feature = "std")]
use neo_store::{ColumnId, Store};
use rand::{rngs::StdRng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

use crate::{
    account::{self, Account},
    error::WalletError,
    nep6::Nep6Contract,
};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScryptParamsConfig {
    pub n: u64,
    pub r: u32,
    pub p: u32,
}

impl From<ScryptParams> for ScryptParamsConfig {
    fn from(value: ScryptParams) -> Self {
        Self {
            n: value.n,
            r: value.r,
            p: value.p,
        }
    }
}

impl From<ScryptParamsConfig> for ScryptParams {
    fn from(value: ScryptParamsConfig) -> Self {
        ScryptParams {
            n: value.n,
            r: value.r,
            p: value.p,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeystoreEntry {
    pub script_hash: Hash160,
    pub cipher_text: Bytes,
    pub salt: [u8; 16],
    pub params: ScryptParamsConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchOnlyEntry {
    pub script_hash: Hash160,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Nep6Contract>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Keystore {
    pub entries: Vec<KeystoreEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_only: Vec<WatchOnlyEntry>,
}

impl Keystore {
    pub fn from_accounts(accounts: &[Account], password: &str) -> Result<Self, WalletError> {
        let mut rng = StdRng::from_entropy();
        let params = ScryptParams {
            n: 1 << 14,
            r: 8,
            p: 8,
        };
        let mut entries = Vec::new();
        let mut watch_only = Vec::new();
        for account in accounts {
            if account.is_watch_only() {
                watch_only.push(WatchOnlyEntry {
                    script_hash: account.script_hash(),
                    contract: account.contract().map(account::contract_to_nep6),
                });
                continue;
            }
            let mut salt = [0u8; 16];
            rng.fill_bytes(&mut salt);
            entries.push(encrypt_account(account, password, params, salt)?);
        }
        Ok(Self { entries, watch_only })
    }

    pub fn unlock(&self, password: &str) -> Result<Vec<PrivateKey>, WalletError> {
        self.entries
            .iter()
            .map(|entry| decrypt_entry(entry, password))
            .collect()
    }
}

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

#[cfg(feature = "std")]
pub fn persist_keystore<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: Vec<u8>,
    keystore: &Keystore,
) -> Result<(), WalletError> {
    let json = serde_json::to_vec(keystore).map_err(|_| WalletError::InvalidKeystore)?;
    store
        .put(column, key, json)
        .map_err(|err| WalletError::Storage(err.to_string()))
}

#[cfg(feature = "std")]
pub fn load_keystore<S: Store + ?Sized>(
    store: &S,
    column: ColumnId,
    key: &[u8],
) -> Result<Option<Keystore>, WalletError> {
    match store
        .get(column, key)
        .map_err(|err| WalletError::Storage(err.to_string()))?
    {
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| WalletError::InvalidKeystore),
        None => Ok(None),
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use hex_literal::hex;
    use neo_store::MemoryStore;

    #[test]
    fn encrypt_and_decrypt_account() {
        let private = PrivateKey::new(hex!(
            "6d16ca2b9f10f8917ac12f90b91f864b0db1d0545d142e9d5b75f1c83c5f4321"
        ));
        let account = Account::from_private_key(private).unwrap();
        let params = ScryptParams {
            n: 1 << 12,
            r: 8,
            p: 1,
        };
        let entry = encrypt_account(&account, "password", params, [7u8; 16]).unwrap();
        let unlocked = decrypt_entry(&entry, "password").unwrap();
        assert_eq!(unlocked.as_be_bytes(), account.private_key_bytes().unwrap());
    }

    #[test]
    fn persist_keystore_roundtrip() {
        let private = PrivateKey::new([3u8; 32]);
        let account = Account::from_private_key(private).unwrap();
        let keystore = Keystore::from_accounts(&[account], "pass").unwrap();
        let store = MemoryStore::new();
        let column = ColumnId::new("keystore");
        persist_keystore(&store, column, b"wallet".to_vec(), &keystore).unwrap();
        let loaded = load_keystore(&store, column, b"wallet").unwrap().unwrap();
        assert_eq!(loaded.entries.len(), keystore.entries.len());
    }
}

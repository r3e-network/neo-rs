use alloc::vec::Vec;

use rand::{rngs::StdRng, RngCore, SeedableRng};
use serde::{Deserialize, Serialize};

use neo_base::{hash::Hash160, Bytes};
use neo_crypto::{ecc256::PrivateKey, scrypt::ScryptParams};

use crate::{
    account::{self, Account},
    error::WalletError,
    nep6::Nep6Contract,
};

use super::crypto::{decrypt_entry, encrypt_account};

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
        Ok(Self {
            entries,
            watch_only,
        })
    }

    pub fn unlock(&self, password: &str) -> Result<Vec<PrivateKey>, WalletError> {
        self.entries
            .iter()
            .map(|entry| decrypt_entry(entry, password))
            .collect()
    }
}

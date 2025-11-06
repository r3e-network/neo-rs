use alloc::{string::String, vec::Vec};

use neo_crypto::scrypt::ScryptParams;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nep6Wallet {
    pub name: String,
    pub version: String,
    pub scrypt: Nep6Scrypt,
    pub accounts: Vec<Nep6Account>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Nep6Scrypt {
    pub n: u64,
    pub r: u32,
    pub p: u32,
}

impl From<ScryptParams> for Nep6Scrypt {
    fn from(value: ScryptParams) -> Self {
        Self {
            n: value.n,
            r: value.r,
            p: value.p,
        }
    }
}

impl From<Nep6Scrypt> for ScryptParams {
    fn from(value: Nep6Scrypt) -> Self {
        ScryptParams {
            n: value.n,
            r: value.r,
            p: value.p,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Nep6Account {
    pub address: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub lock: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract: Option<Nep6Contract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Nep6Contract {
    pub script: String,
    pub parameters: Vec<Nep6Parameter>,
    pub deployed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Nep6Parameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_id: u8,
}

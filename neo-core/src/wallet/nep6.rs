// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

use crate::{types::{Extra, H160}, contract::param::NamedParamType};


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scrypt {
    pub n: u64,
    pub r: u64,
    pub p: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Contract {
    pub script: String,

    pub deployed: bool,

    pub parameters: Vec<NamedParamType>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Account {
    // #[serde(skip)]
    // script_hash: ScriptHash,

    pub address: String,

    pub label: Option<String>,

    #[serde(rename = "isDefault")]
    pub is_default: bool,

    pub lock: bool,

    /// i.e. EncryptedWIF
    pub key: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract: Option<Contract>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Extra,
}

impl Account {
    pub fn is_watch_only(&self) -> bool { self.contract.is_none() }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Nep6Wallet {
    // #[serde(skip)]
    // network: u32,

    pub name: Option<String>,

    pub version: String,

    pub scrypt: Scrypt,

    pub accounts: Vec<Account>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Extra,
}

impl Nep6Wallet {
    pub fn default_account(&self) -> Option<&Account> {
        self.accounts.iter().find(|f| f.is_default)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Token {
    pub name: String,

    pub script_hash: H160,

    pub decimals: u64,

    pub symbol: String,

    pub standard: String,
}


#[cfg(test)]
mod test {
    use super::*;
    use crate::{PublicKey, types::ToNeo3Address, wallet::nep2::Nep2KeyDecrypt};

    #[test]
    #[ignore = "It is too time-consuming"]
    fn test_nep6_wallet() {
        let src = r#"{
    "name": null,
    "version": "3.0",
    "scrypt": { "n": 16384, "r": 8, "p": 8 },
    "accounts": [
        {
            "address": "NPTmAHDxo6Pkyic8Nvu3kwyXoYJCvcCB6i",
            "label": null,
            "isdefault": false,
            "lock": false,
            "key": "6PYUUUFei9PBBfVkSn8q7hFCnewWFRBKPxcn6Kz6Bmk3FqWyLyuTQE2XFH",
            "contract": {
                "script": "DCEDYgBftumtbwC64LbngHbZPDVrSMrEuHXNP0tJzPlOdL5BdHR2qg==",
                "parameters": [{"name": "signature", "type": "Signature"}],
                "deployed": false
            },
            "extra": null
        }
    ],
    "extra": null
}"#;

        let nep6: Nep6Wallet = serde_json::from_str(src)
            .expect("serde-json from_str should be ok");

        assert_eq!(&nep6.version, "3.0");
        assert!(nep6.name.is_none());
        assert!(nep6.extra.is_none());

        let contract = nep6.accounts[0].contract.as_ref().expect("contract should exist");
        assert_eq!(contract.parameters[0].name, "signature");
        assert_eq!(nep6.accounts[0].key, "6PYUUUFei9PBBfVkSn8q7hFCnewWFRBKPxcn6Kz6Bmk3FqWyLyuTQE2XFH");

        let sk = "city of zion".decrypt_nep2_key(&nep6.accounts[0].key)
            .expect("decrypt should be ok");

        let addr = PublicKey::try_from(&sk)
            .expect("to public key should be ok")
            .to_neo3_address();

        assert_eq!(addr.as_str(), &nep6.accounts[0].address);
    }
}
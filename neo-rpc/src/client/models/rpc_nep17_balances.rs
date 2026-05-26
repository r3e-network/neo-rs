// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep17_balances.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::utility::{
    object_array, parse_object_array_lossy, required_address_script_hash, required_bigint_string,
    required_script_hash_or_address, required_u32_number,
};
use neo_core::config::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_json::{JObject, JToken};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// NEP17 balances for an address matching C# `RpcNep17Balances`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balances {
    /// User script hash
    pub user_script_hash: UInt160,

    /// List of token balances
    pub balances: Vec<RpcNep17Balance>,
}

impl RpcNep17Balances {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self, _protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();

        json.insert(
            "balance".to_string(),
            object_array(&self.balances, RpcNep17Balance::to_json),
        );

        json.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &self.user_script_hash,
                _protocol_settings.address_version,
            )),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        let balances = parse_object_array_lossy(json, "balance", |obj| {
            RpcNep17Balance::from_json(obj, _protocol_settings)
        });

        let user_script_hash = required_address_script_hash(json, "address", _protocol_settings)?;

        Ok(Self {
            user_script_hash,
            balances,
        })
    }
}

/// Individual NEP17 balance entry matching C# `RpcNep17Balance`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Balance {
    /// Asset hash
    pub asset_hash: UInt160,

    /// Balance amount
    pub amount: BigInt,

    /// Last updated block height
    pub last_updated_block: u32,
}

impl RpcNep17Balance {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "assethash".to_string(),
            JToken::String(self.asset_hash.to_string()),
        );
        json.insert(
            "amount".to_string(),
            JToken::String(self.amount.to_string()),
        );
        json.insert(
            "lastupdatedblock".to_string(),
            JToken::Number(f64::from(self.last_updated_block)),
        );
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        let asset_hash =
            required_script_hash_or_address(json, "assethash", _protocol_settings, "asset hash")?;
        let amount = required_bigint_string(json, "amount", "amount")?;
        let last_updated_block = required_u32_number(json, "lastupdatedblock")?;

        Ok(Self {
            asset_hash,
            amount,
            last_updated_block,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_json::{JArray, JToken};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn balance_roundtrip() {
        let entry = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(42),
            last_updated_block: 10,
        };
        let json = entry.to_json();
        let parsed =
            RpcNep17Balance::from_json(&json, &ProtocolSettings::default_settings()).unwrap();
        assert_eq!(parsed.asset_hash, entry.asset_hash);
        assert_eq!(parsed.amount, entry.amount);
        assert_eq!(parsed.last_updated_block, entry.last_updated_block);
    }

    #[test]
    fn balances_roundtrip() {
        let entry = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(5),
            last_updated_block: 3,
        };
        let balances = RpcNep17Balances {
            user_script_hash: UInt160::zero(),
            balances: vec![entry.clone()],
        };
        let json = balances.to_json(&ProtocolSettings::default_settings());
        let parsed =
            RpcNep17Balances::from_json(&json, &ProtocolSettings::default_settings()).unwrap();

        assert_eq!(parsed.user_script_hash, balances.user_script_hash);
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].amount, entry.amount);
    }

    #[test]
    fn balances_array_keeps_lossy_parse_behavior() {
        let settings = ProtocolSettings::default_settings();
        let valid = RpcNep17Balance {
            asset_hash: UInt160::zero(),
            amount: BigInt::from(5),
            last_updated_block: 3,
        }
        .to_json();

        let mut malformed = JObject::new();
        malformed.insert(
            "assethash".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );

        let mut balances = JArray::new();
        balances.add(Some(JToken::Object(valid)));
        balances.add(None);
        balances.add(Some(JToken::String("not an object".to_string())));
        balances.add(Some(JToken::Object(malformed)));

        let mut root = JObject::new();
        root.insert("balance".to_string(), JToken::Array(balances));
        root.insert(
            "address".to_string(),
            JToken::String(WalletHelper::to_address(
                &UInt160::zero(),
                settings.address_version,
            )),
        );

        let parsed = RpcNep17Balances::from_json(&root, &settings).unwrap();
        assert_eq!(parsed.balances.len(), 1);
        assert_eq!(parsed.balances[0].amount, BigInt::from(5));
    }

    fn load_rpc_case_result(name: &str) -> Option<JObject> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        if !path.exists() {
            eprintln!(
                "SKIP: neo_csharp submodule not initialized ({})",
                path.display()
            );
            return None;
        }
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token
            .as_array()
            .expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return Some(result.clone());
            }
        }
        eprintln!("SKIP: RpcTestCases.json missing case: {name}");
        None
    }

    #[test]
    fn nep17_balances_to_json_matches_rpc_test_case() {
        let Some(expected) = load_rpc_case_result("getnep17balancesasync") else {
            return;
        };
        let settings = ProtocolSettings::default_settings();
        let parsed = RpcNep17Balances::from_json(&expected, &settings).expect("parse");
        let actual = parsed.to_json(&settings);
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

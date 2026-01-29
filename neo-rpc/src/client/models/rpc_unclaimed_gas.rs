// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_unclaimed_gas.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Unclaimed GAS information matching C# `RpcUnclaimedGas`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUnclaimedGas {
    /// Amount of unclaimed GAS
    pub unclaimed: i64,

    /// Address
    pub address: String,
}

impl RpcUnclaimedGas {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use] 
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "unclaimed".to_string(),
            JToken::String(self.unclaimed.to_string()),
        );
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let unclaimed_token = json
            .get("unclaimed")
            .ok_or("Missing or invalid 'unclaimed' field")?;
        let unclaimed = if let Some(text) = unclaimed_token.as_string() {
            text.parse::<i64>()
                .map_err(|_| format!("Invalid unclaimed value: {text}"))?
        } else if let Some(num) = unclaimed_token.as_number() {
            num as i64
        } else {
            return Err("Invalid 'unclaimed' field".to_string());
        };

        let address = json
            .get("address")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'address' field")?
            ;

        Ok(Self { unclaimed, address })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn rpc_unclaimed_gas_roundtrip() {
        let gas = RpcUnclaimedGas {
            unclaimed: 1234,
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
        };
        let json = gas.to_json();
        let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
        assert_eq!(parsed.unclaimed, gas.unclaimed);
        assert_eq!(parsed.address, gas.address);
    }

    #[test]
    fn rpc_unclaimed_gas_rejects_invalid_value() {
        let mut json = JObject::new();
        json.insert(
            "unclaimed".to_string(),
            JToken::String("not-a-number".into()),
        );
        json.insert(
            "address".to_string(),
            JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
        );
        assert!(RpcUnclaimedGas::from_json(&json).is_err());
    }

    #[test]
    fn rpc_unclaimed_gas_accepts_numeric_value() {
        let mut json = JObject::new();
        json.insert("unclaimed".to_string(), JToken::Number(5f64));
        json.insert(
            "address".to_string(),
            JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
        );
        let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
        assert_eq!(parsed.unclaimed, 5);
    }

    fn load_rpc_case_result(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
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
                return result.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
    }

    #[test]
    fn unclaimed_gas_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("getunclaimedgasasync");
        let parsed = RpcUnclaimedGas::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

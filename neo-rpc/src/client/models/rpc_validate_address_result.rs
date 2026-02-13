// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_validate_address_result.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Address validation result matching C# `RpcValidateAddressResult`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidateAddressResult {
    /// The address that was validated
    pub address: String,

    /// Whether the address is valid
    pub is_valid: bool,
}

impl RpcValidateAddressResult {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("isvalid".to_string(), JToken::Boolean(self.is_valid));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let address = json
            .get("address")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'address' field")?;

        let is_valid = json
            .get("isvalid")
            .map(neo_json::JToken::as_boolean)
            .ok_or("Missing or invalid 'isvalid' field")?;

        Ok(Self { address, is_valid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn validate_address_roundtrip() {
        let result = RpcValidateAddressResult {
            address: "addr".to_string(),
            is_valid: true,
        };
        let json = result.to_json();
        let parsed = RpcValidateAddressResult::from_json(&json).unwrap();
        assert_eq!(parsed.address, result.address);
        assert!(parsed.is_valid);
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
    fn validate_address_to_json_matches_rpc_test_case() {
        let Some(expected) = load_rpc_case_result("validateaddressasync") else {
            return;
        };
        let parsed = RpcValidateAddressResult::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

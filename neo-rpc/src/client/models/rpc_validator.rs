// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_validator.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Validator information matching C# `RpcValidator`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidator {
    /// Validator's public key
    pub public_key: String,

    /// Number of votes for this validator
    pub votes: BigInt,
}

impl RpcValidator {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "publickey".to_string(),
            JToken::String(self.public_key.clone()),
        );
        json.insert("votes".to_string(), JToken::String(self.votes.to_string()));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let public_key = json
            .get("publickey")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'publickey' field")?;

        let votes_token = json
            .get("votes")
            .ok_or("Missing or invalid 'votes' field")?;
        let votes = if let Some(text) = votes_token.as_string() {
            BigInt::from_str(&text).map_err(|_| format!("Invalid votes value: {text}"))?
        } else if let Some(number) = votes_token.as_number() {
            BigInt::from(number as i64)
        } else {
            return Err("Invalid 'votes' field".to_string());
        };

        Ok(Self { public_key, votes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JArray;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn rpc_validator_roundtrip() {
        let validator = RpcValidator {
            public_key: "03abcdef".to_string(),
            votes: BigInt::from(123_456u64),
        };
        let json = validator.to_json();
        let parsed = RpcValidator::from_json(&json).expect("validator");
        assert_eq!(parsed.public_key, validator.public_key);
        assert_eq!(parsed.votes, validator.votes);
    }

    #[test]
    fn rpc_validator_rejects_invalid_votes() {
        let mut json = JObject::new();
        json.insert("publickey".to_string(), JToken::String("03abcdef".into()));
        json.insert("votes".to_string(), JToken::String("not-a-number".into()));

        assert!(RpcValidator::from_json(&json).is_err());
    }

    #[test]
    fn rpc_validator_accepts_numeric_votes() {
        let mut json = JObject::new();
        json.insert("publickey".to_string(), JToken::String("03abcdef".into()));
        json.insert("votes".to_string(), JToken::Number(5f64));

        let parsed = RpcValidator::from_json(&json).expect("validator");
        assert_eq!(parsed.votes, BigInt::from(5));
    }

    fn load_rpc_case_result_array(name: &str) -> Option<JArray> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        if !path.exists() {
            eprintln!("SKIP: neo_csharp submodule not initialized ({})", path.display());
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
                    .and_then(|value| value.as_array())
                    .expect("case result");
                return Some(result.clone());
            }
        }
        eprintln!("SKIP: RpcTestCases.json missing case: {name}");
        None
    }

    #[test]
    fn validators_to_json_matches_rpc_test_case() {
        let Some(expected) = load_rpc_case_result_array("getnextblockvalidatorsasync") else { return; };
        let parsed = expected
            .children()
            .iter()
            .filter_map(|entry| entry.as_ref())
            .filter_map(|token| token.as_object())
            .filter_map(|obj| RpcValidator::from_json(obj).ok())
            .collect::<Vec<_>>();
        let actual = JArray::from(
            parsed
                .iter()
                .map(|validator| JToken::Object(validator.to_json()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

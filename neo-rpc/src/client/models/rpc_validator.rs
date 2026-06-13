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

use super::super::utility::parse_number_or_string_token;
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

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
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or("Missing or invalid 'publickey' field")?;

        let votes_token = json
            .get("votes")
            .ok_or("Missing or invalid 'votes' field")?;
        let votes =
            parse_number_or_string_token(votes_token, "votes", "Invalid 'votes' field", |value| {
                BigInt::from(value as i64)
            })?;

        Ok(Self { public_key, votes })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result_array;
    use super::*;
    use neo_serialization::json::JArray;

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

    #[test]
    fn validators_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result_array("getnextblockvalidatorsasync") else {
            return;
        };
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

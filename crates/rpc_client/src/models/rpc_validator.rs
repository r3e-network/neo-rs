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

/// Validator information matching C# RpcValidator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidator {
    /// Validator's public key
    pub public_key: String,

    /// Number of votes for this validator
    pub votes: BigInt,
}

impl RpcValidator {
    /// Converts to JSON
    /// Matches C# ToJson
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let public_key = json
            .get("publickey")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'publickey' field")?
            .to_string();

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
}

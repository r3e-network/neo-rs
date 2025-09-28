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
        json.insert("publickey".to_string(), JToken::String(self.public_key.clone()));
        json.insert("votes".to_string(), JToken::String(self.votes.to_string()));
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let public_key = json.get("publickey")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'publickey' field")?
            .to_string();
            
        let votes_str = json.get("votes")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'votes' field")?;
        let votes = BigInt::from_str(votes_str)
            .map_err(|_| format!("Invalid votes value: {}", votes_str))?;
            
        Ok(Self {
            public_key,
            votes,
        })
    }
}
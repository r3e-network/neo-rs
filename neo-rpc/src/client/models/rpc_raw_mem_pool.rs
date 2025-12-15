// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_raw_mem_pool.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Raw memory pool information matching C# RpcRawMemPool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRawMemPool {
    /// Current block height
    pub height: u32,

    /// List of verified transaction hashes
    pub verified: Vec<UInt256>,

    /// List of unverified transaction hashes
    pub unverified: Vec<UInt256>,
}

impl RpcRawMemPool {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("height".to_string(), JToken::Number(self.height as f64));

        let verified_array: Vec<JToken> = self
            .verified
            .iter()
            .map(|h| JToken::String(h.to_string()))
            .collect();
        json.insert(
            "verified".to_string(),
            JToken::Array(JArray::from(verified_array)),
        );

        let unverified_array: Vec<JToken> = self
            .unverified
            .iter()
            .map(|h| JToken::String(h.to_string()))
            .collect();
        json.insert(
            "unverified".to_string(),
            JToken::Array(JArray::from(unverified_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let height = if let Some(text) = json.get("height").and_then(|v| v.as_string()) {
            text.parse::<u32>()
                .map_err(|_| format!("Invalid height value: {}", text))?
        } else {
            json.get("height")
                .and_then(|v| v.as_number())
                .ok_or("Missing or invalid 'height' field")? as u32
        };

        let verified = json
            .get("verified")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_string())
                    .filter_map(|s| UInt256::parse(&s).ok())
                    .collect()
            })
            .unwrap_or_default();

        let unverified = json
            .get("unverified")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_string())
                    .filter_map(|s| UInt256::parse(&s).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            height,
            verified,
            unverified,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_mempool_roundtrip() {
        let pool = RpcRawMemPool {
            height: 10,
            verified: vec![UInt256::zero()],
            unverified: vec![UInt256::zero()],
        };
        let json = pool.to_json();
        let parsed = RpcRawMemPool::from_json(&json).unwrap();
        assert_eq!(parsed.height, pool.height);
        assert_eq!(parsed.verified.len(), 1);
        assert_eq!(parsed.unverified.len(), 1);
    }

    #[test]
    fn raw_mempool_accepts_numeric_height() {
        let mut json = JObject::new();
        json.insert("height".to_string(), JToken::Number(5f64));
        json.insert("verified".to_string(), JToken::Array(JArray::new()));
        json.insert("unverified".to_string(), JToken::Array(JArray::new()));

        let parsed = RpcRawMemPool::from_json(&json).unwrap();
        assert_eq!(parsed.height, 5);
    }

    #[test]
    fn raw_mempool_accepts_string_height() {
        let mut json = JObject::new();
        json.insert("height".to_string(), JToken::String("7".to_string()));
        json.insert("verified".to_string(), JToken::Array(JArray::new()));
        json.insert("unverified".to_string(), JToken::Array(JArray::new()));

        let parsed = RpcRawMemPool::from_json(&json).unwrap();
        assert_eq!(parsed.height, 7);
    }
}

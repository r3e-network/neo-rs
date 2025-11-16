// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_raw_mempool.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::UInt256;
use neo_json::{JArray, JObject, JToken};
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
        let height_str = json
            .get("height")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'height' field")?;
        let height = height_str
            .parse::<u32>()
            .map_err(|_| format!("Invalid height value: {}", height_str))?;

        let verified = json
            .get("verified")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_string())
                    .filter_map(|s| UInt256::parse(&s).ok())
                    .collect()
            })
            .unwrap_or_default();

        let unverified = json
            .get("unverified")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_string())
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

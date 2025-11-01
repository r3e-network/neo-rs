// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_found_states.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::JObject;
use serde::{Deserialize, Serialize};

/// Found states result matching C# RpcFoundStates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcFoundStates {
    /// Whether results were truncated
    pub truncated: bool,

    /// Key-value pairs found
    pub results: Vec<(Vec<u8>, Vec<u8>)>,

    /// First proof
    pub first_proof: Option<Vec<u8>>,

    /// Last proof
    pub last_proof: Option<Vec<u8>>,
}

impl RpcFoundStates {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let truncated = json
            .get("truncated")
            .and_then(|v| v.as_boolean())
            .ok_or("Missing or invalid 'truncated' field")?;

        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| {
                        let key = obj
                            .get("key")
                            .and_then(|v| v.as_string())
                            .and_then(|s| base64::decode(s).ok())?;
                        let value = obj
                            .get("value")
                            .and_then(|v| v.as_string())
                            .and_then(|s| base64::decode(s).ok())?;
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let first_proof = json
            .get("firstProof")
            .and_then(|v| v.as_string())
            .and_then(|s| base64::decode(s).ok());

        let last_proof = json
            .get("lastProof")
            .and_then(|v| v.as_string())
            .and_then(|s| base64::decode(s).ok());

        Ok(Self {
            truncated,
            results,
            first_proof,
            last_proof,
        })
    }
}

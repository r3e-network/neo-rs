// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_account.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Account information matching C# RpcAccount
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcAccount {
    /// Account address
    pub address: String,

    /// Whether the account has a key
    pub has_key: bool,

    /// Account label
    pub label: Option<String>,

    /// Whether this is a watch-only account
    pub watch_only: bool,
}

impl RpcAccount {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("haskey".to_string(), JToken::Boolean(self.has_key));

        if let Some(ref label) = self.label {
            json.insert("label".to_string(), JToken::String(label.clone()));
        }

        json.insert("watchonly".to_string(), JToken::Boolean(self.watch_only));
        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?
            .to_string();

        let has_key = json
            .get("haskey")
            .map(|v| v.as_boolean())
            .ok_or("Missing or invalid 'haskey' field")?;

        let label = json
            .get("label")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let watch_only = json
            .get("watchonly")
            .map(|v| v.as_boolean())
            .ok_or("Missing or invalid 'watchonly' field")?;

        Ok(Self {
            address,
            has_key,
            label,
            watch_only,
        })
    }
}

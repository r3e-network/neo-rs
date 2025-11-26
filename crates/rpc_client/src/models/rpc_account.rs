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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_account_roundtrip_with_label() {
        let account = RpcAccount {
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
            has_key: true,
            label: Some("main".to_string()),
            watch_only: false,
        };

        let json = account.to_json();
        let parsed = RpcAccount::from_json(&json).expect("account");
        assert_eq!(parsed.address, account.address);
        assert_eq!(parsed.has_key, account.has_key);
        assert_eq!(parsed.label, account.label);
        assert_eq!(parsed.watch_only, account.watch_only);
    }

    #[test]
    fn rpc_account_roundtrip_without_label() {
        let account = RpcAccount {
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
            has_key: false,
            label: None,
            watch_only: true,
        };

        let json = account.to_json();
        assert!(json.get("label").is_none());
        let parsed = RpcAccount::from_json(&json).expect("account");
        assert!(parsed.label.is_none());
        assert!(parsed.watch_only);
    }
}

// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_plugin.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use serde::{Deserialize, Serialize};

/// Plugin information matching C# RpcPlugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPlugin {
    /// Plugin name
    pub name: String,

    /// Plugin version
    pub version: String,

    /// Interfaces implemented by the plugin
    pub interfaces: Vec<String>,
}

impl RpcPlugin {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("name".to_string(), JToken::String(self.name.clone()));
        json.insert("version".to_string(), JToken::String(self.version.clone()));

        let interfaces_array: Vec<JToken> = self
            .interfaces
            .iter()
            .map(|s| JToken::String(s.clone()))
            .collect();
        json.insert(
            "interfaces".to_string(),
            JToken::Array(JArray::from(interfaces_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let name = json
            .get("name")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'name' field")?
            .to_string();

        let version = json
            .get("version")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'version' field")?
            .to_string();

        let interfaces = json
            .get("interfaces")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_string())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            name,
            version,
            interfaces,
        })
    }
}

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

    /// Optional category provided by newer nodes (e.g., "Consensus", "Rpc").
    #[serde(default)]
    pub category: Option<String>,
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
        if let Some(category) = &self.category {
            json.insert("category".to_string(), JToken::String(category.clone()));
        }

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
                    .map(|item| {
                        item.as_ref()
                            .and_then(|token| token.as_string())
                            .map(|s| s.to_string())
                            .ok_or_else(|| "Interface entry must be a string".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_else(|| Ok(Vec::new()))?;

        let category = json
            .get("category")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        Ok(Self {
            name,
            version,
            interfaces,
            category,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_plugin_roundtrip() {
        let plugin = RpcPlugin {
            name: "RpcServer".into(),
            version: "1.0.0".into(),
            interfaces: vec!["ISmartContract".into(), "IBlock".into()],
            category: Some("Rpc".into()),
        };

        let json = plugin.to_json();
        let parsed = RpcPlugin::from_json(&json).expect("plugin");
        assert_eq!(parsed.name, plugin.name);
        assert_eq!(parsed.version, plugin.version);
        assert_eq!(parsed.interfaces, plugin.interfaces);
        assert_eq!(parsed.category, plugin.category);
    }

    #[test]
    fn rpc_plugin_defaults_to_empty_interfaces() {
        let mut json = JObject::new();
        json.insert("name".to_string(), JToken::String("Empty".into()));
        json.insert("version".to_string(), JToken::String("0.0.1".into()));

        let parsed = RpcPlugin::from_json(&json).expect("plugin");
        assert!(parsed.interfaces.is_empty());
        assert!(parsed.category.is_none());
    }
}

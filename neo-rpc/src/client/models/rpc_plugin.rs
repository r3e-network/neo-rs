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

use super::super::utility::{parse_optional_string_array_strict, token_array};
use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Plugin information matching C# `RpcPlugin`
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
    pub category: Option<String>}

impl RpcPlugin {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("name".to_string(), JToken::String(self.name.clone()));
        json.insert("version".to_string(), JToken::String(self.version.clone()));

        json.insert(
            "interfaces".to_string(),
            token_array(&self.interfaces, |name| JToken::String(name.clone())),
        );
        if let Some(category) = &self.category {
            json.insert("category".to_string(), JToken::String(category.clone()));
       }

        json
   }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let name = json
            .get("name")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'name' field")?;

        let version = json
            .get("version")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'version' field")?;

        let interfaces = parse_optional_string_array_strict(
            json,
            "interfaces",
            "Interface entry must be a string",
        )?;

        let category = json.get("category").and_then(neo_json::JToken::as_string);

        Ok(Self {
            name,
            version,
            interfaces,
            category})
   }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_fixtures::rpc_case_result_array;
    use neo_json::{JArray, JToken};

    #[test]
    fn rpc_plugin_roundtrip() {
        let plugin = RpcPlugin {
            name: "RpcServer".into(),
            version: "1.0.0".into(),
            interfaces: vec!["ISmartContract".into(), "IBlock".into()],
            category: Some("Rpc".into())};

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

        json.insert("interfaces".to_string(), JToken::Boolean(true));
        let parsed = RpcPlugin::from_json(&json).expect("plugin");
        assert!(parsed.interfaces.is_empty());
   }

    #[test]
    fn rpc_plugin_rejects_empty_or_non_string_interface_entries() {
        let mut json = JObject::new();
        json.insert("name".to_string(), JToken::String("Bad".into()));
        json.insert("version".to_string(), JToken::String("0.0.1".into()));

        let mut empty_slot = JArray::new();
        empty_slot.add(None);
        json.insert("interfaces".to_string(), JToken::Array(empty_slot));
        let err = RpcPlugin::from_json(&json).expect_err("empty slot should fail");
        assert_eq!(err, "Interface entry must be a string");

        json.insert(
            "interfaces".to_string(),
            JToken::Array(JArray::from(vec![JToken::Number(1.0)])),
        );
        let err = RpcPlugin::from_json(&json).expect_err("non-string should fail");
        assert_eq!(err, "Interface entry must be a string");
   }

    #[test]
    fn plugins_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result_array("listpluginsasync") else {
            return;
       };
        let parsed = expected
            .children()
            .iter()
            .filter_map(|entry| entry.as_ref())
            .filter_map(|token| token.as_object())
            .filter_map(|obj| RpcPlugin::from_json(obj).ok())
            .collect::<Vec<_>>();
        let actual = JArray::from(
            parsed
                .iter()
                .map(|plugin| JToken::Object(plugin.to_json()))
                .collect::<Vec<_>>(),
        );
        assert_eq!(expected.to_string(), actual.to_string());
   }
}

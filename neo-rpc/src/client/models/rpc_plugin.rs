use super::super::utility::{parse_optional_string_array_strict, token_array};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
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
    pub category: Option<String>,
}

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
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let name = json
            .get("name")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'name' field"))?;

        let version = json
            .get("version")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'version' field"))?;

        let interfaces = parse_optional_string_array_strict(
            json,
            "interfaces",
            "Interface entry must be a string",
        )?;

        let category = json
            .get("category")
            .and_then(neo_serialization::json::JToken::as_string);

        Ok(Self {
            name,
            version,
            interfaces,
            category,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_plugin.rs"]
mod tests;

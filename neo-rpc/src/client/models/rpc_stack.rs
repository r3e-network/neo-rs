//! RPC stack item representation (`RpcStack`).

use neo_serialization::json::{JObject, JToken};

/// RPC stack item representation matching C# `RpcStack`
#[derive(Debug, Clone)]
pub struct RpcStack {
    /// Stack item type
    pub item_type: String,

    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let item_type = json
            .get("type")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or("Missing or invalid 'type' field")?;

        let value = json.get("value").ok_or("Missing 'value' field")?.clone();

        Ok(Self { item_type, value })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("type".to_string(), JToken::String(self.item_type.clone()));
        json.insert("value".to_string(), self.value.clone());
        json
    }
}

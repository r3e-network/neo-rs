use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Address validation result matching C# `RpcValidateAddressResult`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidateAddressResult {
    /// The address that was validated
    pub address: String,

    /// Whether the address is valid
    pub is_valid: bool,
}

impl RpcValidateAddressResult {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("isvalid".to_string(), JToken::Boolean(self.is_valid));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        let is_valid = json
            .get("isvalid")
            .map(neo_serialization::json::JToken::as_boolean)
            .ok_or_else(|| CoreError::other("Missing or invalid 'isvalid' field"))?;

        Ok(Self { address, is_valid })
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_validate_address_result.rs"]
mod tests;

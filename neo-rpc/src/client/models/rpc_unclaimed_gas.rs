use super::super::utility::parse_number_or_string_token;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Unclaimed GAS information matching C# `RpcUnclaimedGas`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUnclaimedGas {
    /// Amount of unclaimed GAS
    pub unclaimed: i64,

    /// Address
    pub address: String,
}

impl RpcUnclaimedGas {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "unclaimed".to_string(),
            JToken::String(self.unclaimed.to_string()),
        );
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let unclaimed_token = json
            .get("unclaimed")
            .ok_or_else(|| CoreError::other("Missing or invalid 'unclaimed' field"))?;
        let unclaimed = parse_number_or_string_token(
            unclaimed_token,
            "unclaimed",
            "Invalid 'unclaimed' field",
            |value| value as i64,
        )
        .map_err(|e| CoreError::other(e.to_string()))?;

        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        Ok(Self { unclaimed, address })
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_unclaimed_gas.rs"]
mod tests;

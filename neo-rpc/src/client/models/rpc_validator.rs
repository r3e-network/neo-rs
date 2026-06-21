use super::super::utility::parse_number_or_string_token;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

/// Validator information matching C# `RpcValidator`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidator {
    /// Validator's public key
    pub public_key: String,

    /// Number of votes for this validator
    pub votes: BigInt,
}

impl RpcValidator {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "publickey".to_string(),
            JToken::String(self.public_key.clone()),
        );
        json.insert("votes".to_string(), JToken::String(self.votes.to_string()));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let public_key = json
            .get("publickey")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'publickey' field"))?;

        let votes_token = json
            .get("votes")
            .ok_or_else(|| CoreError::other("Missing or invalid 'votes' field"))?;
        let votes =
            parse_number_or_string_token(votes_token, "votes", "Invalid 'votes' field", |value| {
                BigInt::from(value as i64)
            })
            .map_err(|e| CoreError::other(e.to_string()))?;

        Ok(Self { public_key, votes })
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_validator.rs"]
mod tests;

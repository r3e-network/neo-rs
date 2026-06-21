use super::super::utility::{base64_string_token, object_array, optional_base64_field_lossy};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Found states result matching C# `RpcFoundStates`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcFoundStates {
    /// Whether results were truncated
    pub truncated: bool,

    /// Key-value pairs found
    pub results: Vec<(Vec<u8>, Vec<u8>)>,

    /// First proof
    pub first_proof: Option<Vec<u8>>,

    /// Last proof
    pub last_proof: Option<Vec<u8>>,
}

impl RpcFoundStates {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let truncated = json
            .get("truncated")
            .map(neo_serialization::json::JToken::as_boolean)
            .ok_or_else(|| CoreError::other("Missing or invalid 'truncated' field"))?;

        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| {
                        let key = optional_base64_field_lossy(obj, "key")?;
                        let value = optional_base64_field_lossy(obj, "value")?;
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let first_proof = optional_base64_field_lossy(json, "firstProof");
        let last_proof = optional_base64_field_lossy(json, "lastProof");

        Ok(Self {
            truncated,
            results,
            first_proof,
            last_proof,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("truncated".to_string(), JToken::Boolean(self.truncated));

        json.insert(
            "results".to_string(),
            object_array(&self.results, |(key, value)| {
                let mut entry = JObject::new();
                entry.insert("key".to_string(), base64_string_token(key));
                entry.insert("value".to_string(), base64_string_token(value));
                entry
            }),
        );

        if let Some(first) = &self.first_proof {
            json.insert("firstProof".to_string(), base64_string_token(first));
        }
        if let Some(last) = &self.last_proof {
            json.insert("lastProof".to_string(), base64_string_token(last));
        }

        json
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_found_states.rs"]
mod tests;

use super::super::utility::cloned_token_array;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// RPC request structure matching C# `RpcRequest`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Request ID
    pub id: JToken,

    /// JSON-RPC version
    #[serde(rename = "jsonrpc")]
    pub json_rpc: String,

    /// Method name
    pub method: String,

    /// Method parameters
    pub params: Vec<JToken>,
}

impl RpcRequest {
    /// Creates a new RPC request
    #[must_use]
    pub fn new(id: JToken, method: String, params: Vec<JToken>) -> Self {
        Self {
            id,
            json_rpc: "2.0".to_string(),
            method,
            params,
        }
    }

    /// Creates an RPC request from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let id = json
            .get("id")
            .ok_or_else(|| CoreError::other("Missing 'id' field"))?
            .clone();

        let json_rpc = json
            .get("jsonrpc")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'jsonrpc' field"))?;

        let method = json
            .get("method")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'method' field"))?;

        let params = json
            .get("params")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(std::clone::Clone::clone)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(Self {
            id,
            json_rpc,
            method,
            params,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("id".to_string(), self.id.clone());
        json.insert("jsonrpc".to_string(), JToken::String(self.json_rpc.clone()));
        json.insert("method".to_string(), JToken::String(self.method.clone()));
        json.insert("params".to_string(), cloned_token_array(&self.params));
        json
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/core/rpc_request.rs"]
mod tests;

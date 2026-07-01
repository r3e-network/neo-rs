use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// RPC response structure matching C# `RpcResponse`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Response ID
    pub id: JToken,

    /// JSON-RPC version
    #[serde(rename = "jsonrpc")]
    pub json_rpc: String,

    /// Error if present
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcResponseError>,

    /// Result if successful
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JToken>,

    /// Raw response string
    #[serde(skip)]
    pub raw_response: Option<String>,
}

impl RpcResponse {
    /// Creates an RPC response from JSON
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

        let result = json.get("result").cloned();

        let error = json
            .get("error")
            .and_then(|e| e.as_object())
            .and_then(|obj| RpcResponseError::from_json(obj).ok());

        Ok(Self {
            id,
            json_rpc,
            error,
            result,
            raw_response: None,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("id".to_string(), self.id.clone());
        json.insert("jsonrpc".to_string(), JToken::String(self.json_rpc.clone()));

        let error = self
            .error
            .as_ref()
            .map_or(JToken::Null, |value| JToken::Object(value.to_json()));
        json.insert("error".to_string(), error);

        let result = self.result.clone().unwrap_or(JToken::Null);
        json.insert("result".to_string(), result);

        json
    }
}

/// RPC response error structure matching C# `RpcResponseError`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponseError {
    /// Error code
    pub code: i32,

    /// Error message
    pub message: String,

    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<JToken>,
}

impl RpcResponseError {
    /// Creates an RPC response error from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let code = json
            .get("code")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'code' field"))?
            as i32;

        let message = json
            .get("message")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'message' field"))?;

        let data = json.get("data").cloned();

        Ok(Self {
            code,
            message,
            data,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("code".to_string(), JToken::Number(f64::from(self.code)));
        json.insert("message".to_string(), JToken::String(self.message.clone()));

        let data = self.data.clone().unwrap_or(JToken::Null);
        json.insert("data".to_string(), data);

        json
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/core/rpc_response.rs"]
mod tests;

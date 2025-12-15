// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_response.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// RPC response structure matching C# RpcResponse
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let id = json.get("id").ok_or("Missing 'id' field")?.clone();

        let json_rpc = json
            .get("jsonrpc")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'jsonrpc' field")?
            .to_string();

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
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("id".to_string(), self.id.clone());
        json.insert("jsonrpc".to_string(), JToken::String(self.json_rpc.clone()));

        if let Some(ref error) = self.error {
            json.insert("error".to_string(), JToken::Object(error.to_json()));
        }

        if let Some(ref result) = self.result {
            json.insert("result".to_string(), result.clone());
        }

        json
    }
}

/// RPC response error structure matching C# RpcResponseError
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let code = json
            .get("code")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'code' field")? as i32;

        let message = json
            .get("message")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'message' field")?
            .to_string();

        let data = json.get("data").cloned();

        Ok(Self {
            code,
            message,
            data,
        })
    }

    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("code".to_string(), JToken::Number(self.code as f64));
        json.insert("message".to_string(), JToken::String(self.message.clone()));

        if let Some(ref data) = self.data {
            json.insert("data".to_string(), data.clone());
        }

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JToken;

    #[test]
    fn rpc_response_roundtrip_success() {
        let resp = RpcResponse {
            id: JToken::Number(1f64),
            json_rpc: "2.0".to_string(),
            error: None,
            result: Some(JToken::String("ok".to_string())),
            raw_response: None,
        };
        let json = resp.to_json();
        let parsed = RpcResponse::from_json(&json).unwrap();
        assert_eq!(parsed.json_rpc, resp.json_rpc);
        assert!(parsed.error.is_none());
        assert_eq!(parsed.result.as_ref().unwrap().as_string().unwrap(), "ok");
    }

    #[test]
    fn rpc_response_roundtrip_error() {
        let err = RpcResponseError {
            code: -1,
            message: "bad".to_string(),
            data: Some(JToken::String("info".to_string())),
        };
        let mut json = JObject::new();
        json.insert("id".to_string(), JToken::Null);
        json.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
        json.insert("error".to_string(), JToken::Object(err.to_json()));

        let parsed = RpcResponse::from_json(&json).unwrap();
        let parsed_err = parsed.error.unwrap();
        assert_eq!(parsed_err.code, err.code);
        assert_eq!(parsed_err.message, err.message);
        assert_eq!(parsed_err.data.unwrap().as_string().unwrap(), "info");
    }

    #[test]
    fn rpc_response_to_json_with_result_and_error_data() {
        let resp = RpcResponse {
            id: JToken::String("1".into()),
            json_rpc: "2.0".into(),
            error: Some(RpcResponseError {
                code: -32000,
                message: "failure".into(),
                data: Some(JToken::String("details".into())),
            }),
            result: Some(JToken::String("ignored".into())),
            raw_response: None,
        };

        let json = resp.to_json();
        let parsed = RpcResponse::from_json(&json).unwrap();
        let err = parsed.error.unwrap();
        assert_eq!(err.code, -32000);
        assert_eq!(err.message, "failure");
        assert_eq!(err.data.unwrap().as_string().unwrap(), "details");
        assert_eq!(parsed.result.unwrap().as_string().unwrap(), "ignored");
    }
}

// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_request.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use serde::{Deserialize, Serialize};

/// RPC request structure matching C# RpcRequest
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
    pub fn new(id: JToken, method: String, params: Vec<JToken>) -> Self {
        Self {
            id,
            json_rpc: "2.0".to_string(),
            method,
            params,
        }
    }

    /// Creates an RPC request from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let id = json.get("id").ok_or("Missing 'id' field")?.clone();

        let json_rpc = json
            .get("jsonrpc")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'jsonrpc' field")?
            .to_string();

        let method = json
            .get("method")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'method' field")?
            .to_string();

        let params = json
            .get("params")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().cloned().collect())
            .unwrap_or_default();

        Ok(Self {
            id,
            json_rpc,
            method,
            params,
        })
    }

    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("id".to_string(), self.id.clone());
        json.insert("jsonrpc".to_string(), JToken::String(self.json_rpc.clone()));
        json.insert("method".to_string(), JToken::String(self.method.clone()));
        json.insert(
            "params".to_string(),
            JToken::Array(JArray::from(self.params.clone())),
        );
        json
    }
}

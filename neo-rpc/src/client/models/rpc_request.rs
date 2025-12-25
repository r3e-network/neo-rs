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
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JArray;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn rpc_request_roundtrip() {
        let req = RpcRequest {
            id: JToken::Number(7f64),
            json_rpc: "2.0".to_string(),
            method: "getblock".to_string(),
            params: vec![JToken::String("0xabc".to_string())],
        };
        let json = req.to_json();
        let parsed = RpcRequest::from_json(&json).unwrap();
        assert_eq!(parsed.id.as_number(), Some(7f64));
        assert_eq!(parsed.json_rpc, req.json_rpc);
        assert_eq!(parsed.method, req.method);
        assert_eq!(parsed.params.len(), 1);
    }

    #[test]
    fn rpc_request_defaults_params_and_accepts_string_id() {
        let mut json = JObject::new();
        json.insert("id".to_string(), JToken::String("abc".to_string()));
        json.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
        json.insert(
            "method".to_string(),
            JToken::String("getversion".to_string()),
        );

        let parsed = RpcRequest::from_json(&json).unwrap();
        assert_eq!(parsed.id.as_string().unwrap(), "abc");
        assert!(parsed.params.is_empty());
    }

    fn load_rpc_case_request(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("tests");
        path.push("Neo.RpcClient.Tests");
        path.push("RpcTestCases.json");
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token.as_array().expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let request = obj
                    .get("Request")
                    .and_then(|value| value.as_object())
                    .expect("case request");
                return request.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
    }

    fn build_expected_request(request: &JObject) -> JObject {
        let mut expected = JObject::new();
        expected.insert(
            "id".to_string(),
            request.get("id").cloned().unwrap_or(JToken::Null),
        );
        expected.insert(
            "jsonrpc".to_string(),
            request
                .get("jsonrpc")
                .cloned()
                .unwrap_or(JToken::String("2.0".into())),
        );
        expected.insert(
            "method".to_string(),
            request
                .get("method")
                .cloned()
                .unwrap_or(JToken::String(String::new())),
        );
        expected.insert(
            "params".to_string(),
            request
                .get("params")
                .cloned()
                .unwrap_or(JToken::Array(JArray::new())),
        );
        expected
    }

    #[test]
    fn request_to_json_matches_rpc_test_case_with_params() {
        let request = load_rpc_case_request("sendrawtransactionasyncerror");
        let expected = build_expected_request(&request);
        let parsed = RpcRequest::from_json(&request).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }

    #[test]
    fn request_to_json_matches_rpc_test_case_without_params() {
        let request = load_rpc_case_request("getbestblockhashasync");
        let expected = build_expected_request(&request);
        let parsed = RpcRequest::from_json(&request).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

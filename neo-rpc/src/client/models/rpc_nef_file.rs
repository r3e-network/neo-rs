// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nef_file.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::utility::{object_array, parse_object_array_lossy};
use super::RpcMethodToken;
use base64::{Engine as _, engine::general_purpose};
use neo_core::smart_contract::NefFile;
use neo_json::{JObject, JToken};

/// RPC NEF file helper matching C# `RpcNefFile`
pub struct RpcNefFile {
    /// The NEF file
    pub nef_file: NefFile,
}

impl RpcNefFile {
    /// Creates a new wrapper from a NEF file
    #[must_use]
    pub const fn new(nef_file: NefFile) -> Self {
        Self { nef_file }
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let compiler = json
            .get("compiler")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'compiler' field")?;

        let source = json
            .get("source")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'source' field")?;

        let tokens = parse_object_array_lossy(json, "tokens", RpcMethodToken::from_json);

        let script = json
            .get("script")
            .and_then(neo_json::JToken::as_string)
            .and_then(|s| general_purpose::STANDARD.decode(s).ok())
            .ok_or("Missing or invalid 'script' field")?;

        let checksum = json
            .get("checksum")
            .and_then(neo_json::JToken::as_number)
            .ok_or("Missing or invalid 'checksum' field")? as u32;

        Ok(Self {
            nef_file: NefFile {
                compiler,
                source,
                tokens: tokens.into_iter().map(|t| t.method_token).collect(),
                script,
                checksum,
            },
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "magic".to_string(),
            JToken::Number(f64::from(NefFile::MAGIC)),
        );
        json.insert(
            "compiler".to_string(),
            JToken::String(self.nef_file.compiler.clone()),
        );
        json.insert(
            "source".to_string(),
            JToken::String(self.nef_file.source.clone()),
        );
        json.insert(
            "tokens".to_string(),
            object_array(&self.nef_file.tokens, |t| {
                RpcMethodToken {
                    method_token: t.clone(),
                }
                .to_json()
            }),
        );
        json.insert(
            "script".to_string(),
            JToken::String(general_purpose::STANDARD.encode(&self.nef_file.script)),
        );
        json.insert(
            "checksum".to_string(),
            JToken::Number(f64::from(self.nef_file.checksum)),
        );
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::smart_contract::method_token::MethodToken;
    use std::fs;
    use std::path::PathBuf;

    fn sample_nef() -> NefFile {
        NefFile {
            compiler: "neo".into(),
            source: "src".into(),
            tokens: vec![MethodToken::default()],
            script: vec![1, 2, 3],
            checksum: 999,
        }
    }

    #[test]
    fn rpc_nef_file_roundtrip() {
        let nef = sample_nef();
        let rpc = RpcNefFile::new(nef.clone());
        let json = rpc.to_json();
        let parsed = RpcNefFile::from_json(&json).expect("nef");
        assert_eq!(parsed.nef_file.compiler, nef.compiler);
        assert_eq!(parsed.nef_file.tokens.len(), nef.tokens.len());
        assert_eq!(parsed.nef_file.script, nef.script);
        assert_eq!(parsed.nef_file.checksum, nef.checksum);
    }

    #[test]
    fn rpc_nef_file_rejects_missing_script() {
        let mut json = JObject::new();
        json.insert("compiler".to_string(), JToken::String("neo".into()));
        json.insert("source".to_string(), JToken::String("src".into()));
        json.insert("tokens".to_string(), JToken::Array(neo_json::JArray::new()));
        json.insert("checksum".to_string(), JToken::Number(1f64));

        assert!(RpcNefFile::from_json(&json).is_err());
    }

    fn load_rpc_case_result(name: &str) -> Option<JObject> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        if !path.exists() {
            eprintln!(
                "SKIP: neo_csharp submodule not initialized ({})",
                path.display()
            );
            return None;
        }
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token
            .as_array()
            .expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                let nef = result
                    .get("nef")
                    .and_then(|value| value.as_object())
                    .expect("nef result");
                return Some(nef.clone());
            }
        }
        eprintln!("SKIP: RpcTestCases.json missing case: {name}");
        None
    }

    #[test]
    fn nef_to_json_matches_rpc_test_case() {
        let Some(expected) = load_rpc_case_result("getcontractstateasync") else {
            return;
        };
        let parsed = RpcNefFile::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

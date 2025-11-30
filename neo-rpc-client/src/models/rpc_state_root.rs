// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_state_root.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt256, Witness};
use neo_json::{JArray, JObject, JToken};
use serde::{Deserialize, Serialize};

/// State root information matching C# RpcStateRoot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStateRoot {
    /// Version
    pub version: u8,

    /// Index
    pub index: u32,

    /// Root hash
    pub root_hash: UInt256,

    /// Witness
    pub witness: Option<Witness>,
}

impl RpcStateRoot {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let version = json
            .get("version")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'version' field")? as u8;

        let index = json
            .get("index")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'index' field")? as u32;

        let root_hash = json
            .get("roothash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(&s).ok())
            .ok_or("Missing or invalid 'roothash' field")?;

        let witness = json
            .get("witnesses")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().next())
            .and_then(|entry| entry.as_ref())
            .and_then(|token| token.as_object())
            .and_then(|obj| crate::utility::witness_from_json(obj).ok());

        Ok(Self {
            version,
            index,
            root_hash,
            witness,
        })
    }

    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("version".to_string(), JToken::Number(self.version as f64));
        json.insert("index".to_string(), JToken::Number(self.index as f64));
        json.insert(
            "roothash".to_string(),
            JToken::String(self.root_hash.to_string()),
        );

        if let Some(witness) = &self.witness {
            let witness_json = crate::utility::witness_to_json(witness);
            json.insert(
                "witnesses".to_string(),
                JToken::Array(JArray::from(vec![JToken::Object(witness_json)])),
            );
        }

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use neo_json::JArray;

    #[test]
    fn rpc_state_root_parses_with_witness() {
        let mut json = JObject::new();
        json.insert("version".to_string(), neo_json::JToken::Number(0f64));
        json.insert("index".to_string(), neo_json::JToken::Number(1f64));
        json.insert(
            "roothash".to_string(),
            neo_json::JToken::String(UInt256::zero().to_string()),
        );

        let mut witness_obj = JObject::new();
        witness_obj.insert(
            "invocation".to_string(),
            neo_json::JToken::String(general_purpose::STANDARD.encode(b"i")),
        );
        witness_obj.insert(
            "verification".to_string(),
            neo_json::JToken::String(general_purpose::STANDARD.encode(b"v")),
        );
        json.insert(
            "witnesses".to_string(),
            neo_json::JToken::Array(JArray::from(vec![neo_json::JToken::Object(witness_obj)])),
        );

        let parsed = RpcStateRoot::from_json(&json).expect("state root");
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.index, 1);
        assert_eq!(parsed.root_hash, UInt256::zero());
        let witness = parsed.witness.expect("witness");
        assert_eq!(witness.invocation_script(), b"i");
        assert_eq!(witness.verification_script(), b"v");
    }

    #[test]
    fn rpc_state_root_allows_missing_witness() {
        let mut json = JObject::new();
        json.insert("version".to_string(), neo_json::JToken::Number(0f64));
        json.insert("index".to_string(), neo_json::JToken::Number(1f64));
        json.insert(
            "roothash".to_string(),
            neo_json::JToken::String(UInt256::zero().to_string()),
        );

        let parsed = RpcStateRoot::from_json(&json).expect("state root");
        assert!(parsed.witness.is_none());
    }

    #[test]
    fn rpc_state_root_roundtrip() {
        let root = RpcStateRoot {
            version: 1,
            index: 10,
            root_hash: UInt256::zero(),
            witness: None,
        };
        let json = root.to_json();
        let parsed = RpcStateRoot::from_json(&json).expect("state root");
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.index, 10);
    }

    #[test]
    fn rpc_state_root_roundtrip_with_witness_json() {
        let witness = crate::utility::witness_from_json(&{
            let mut obj = JObject::new();
            obj.insert(
                "invocation".to_string(),
                JToken::String(general_purpose::STANDARD.encode(b"i")),
            );
            obj.insert(
                "verification".to_string(),
                JToken::String(general_purpose::STANDARD.encode(b"v")),
            );
            obj
        })
        .unwrap();

        let root = RpcStateRoot {
            version: 2,
            index: 11,
            root_hash: UInt256::zero(),
            witness: Some(witness),
        };
        let json = root.to_json();
        let parsed = RpcStateRoot::from_json(&json).expect("state root");
        assert!(parsed.witness.is_some());
        let parsed_witness = parsed.witness.unwrap();
        assert_eq!(parsed_witness.invocation_script(), b"i");
        assert_eq!(parsed_witness.verification_script(), b"v");
    }
}

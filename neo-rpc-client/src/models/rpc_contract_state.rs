// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_contract_state.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::RpcNefFile;
use neo_core::{ContractManifest, ContractState, UInt160};
use neo_json::{JObject, JToken};

/// RPC contract state information matching C# RpcContractState
#[derive(Debug, Clone)]
pub struct RpcContractState {
    /// The contract state
    pub contract_state: ContractState,
}

impl RpcContractState {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let id = json
            .get("id")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'id' field")? as i32;

        let update_counter = json
            .get("updatecounter")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'updatecounter' field")? as u16;

        let hash = json
            .get("hash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(&s).ok())
            .ok_or("Missing or invalid 'hash' field")?;

        let nef_json = json
            .get("nef")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'nef' field")?;
        let nef = RpcNefFile::from_json(nef_json)?;

        let manifest_json = json
            .get("manifest")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'manifest' field")?;
        let manifest_str = manifest_json.to_string();
        let manifest = ContractManifest::from_json(&manifest_str)
            .map_err(|err| format!("Invalid manifest: {err}"))?;

        Ok(Self {
            contract_state: ContractState {
                id,
                update_counter,
                hash,
                nef: nef.nef_file,
                manifest,
            },
        })
    }

    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> Result<JObject, String> {
        let mut json = JObject::new();
        json.insert(
            "id".to_string(),
            JToken::Number(self.contract_state.id as f64),
        );
        json.insert(
            "updatecounter".to_string(),
            JToken::Number(self.contract_state.update_counter as f64),
        );
        json.insert(
            "hash".to_string(),
            JToken::String(self.contract_state.hash.to_string()),
        );
        json.insert(
            "nef".to_string(),
            JToken::Object(
                RpcNefFile {
                    nef_file: self.contract_state.nef.clone(),
                }
                .to_json(),
            ),
        );

        let manifest_json_value = self
            .contract_state
            .manifest
            .to_json()
            .map_err(|err| err.to_string())?;
        let manifest_jtoken = neo_json::JToken::parse(&manifest_json_value.to_string(), 128)
            .map_err(|err| err.to_string())?;
        json.insert("manifest".to_string(), manifest_jtoken);

        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use neo_core::smart_contract::manifest::ContractManifest;
    use neo_core::smart_contract::NefFile;
    use neo_json::{JArray, JToken};

    #[test]
    fn rpc_contract_state_parses_minimal_contract() {
        let mut json = JObject::new();
        json.insert("id".to_string(), JToken::Number(1f64));
        json.insert("updatecounter".to_string(), JToken::Number(2f64));
        json.insert(
            "hash".to_string(),
            JToken::String(UInt160::zero().to_string()),
        );

        let nef = NefFile {
            compiler: "neo".into(),
            source: "".into(),
            tokens: Vec::new(),
            script: vec![1, 2, 3],
            checksum: 123,
        };
        let mut nef_json = JObject::new();
        nef_json.insert("compiler".to_string(), JToken::String(nef.compiler.clone()));
        nef_json.insert("source".to_string(), JToken::String(nef.source.clone()));
        nef_json.insert("tokens".to_string(), JToken::Array(JArray::new()));
        nef_json.insert(
            "script".to_string(),
            JToken::String(general_purpose::STANDARD.encode(&nef.script)),
        );
        nef_json.insert("checksum".to_string(), JToken::Number(nef.checksum as f64));
        json.insert("nef".to_string(), JToken::Object(nef_json));

        let manifest = ContractManifest::new("TestContract".into());
        let manifest_value = manifest.to_json().expect("manifest json");
        let manifest_token =
            JToken::parse(&manifest_value.to_string(), 128).expect("neo-json parse");
        json.insert(
            "manifest".to_string(),
            JToken::Object(manifest_token.as_object().unwrap().clone()),
        );

        let parsed = RpcContractState::from_json(&json).expect("contract state");
        assert_eq!(parsed.contract_state.id, 1);
        assert_eq!(parsed.contract_state.update_counter, 2);
        assert_eq!(parsed.contract_state.hash, UInt160::zero());
        assert_eq!(parsed.contract_state.nef.checksum, 123);
        assert_eq!(parsed.contract_state.manifest.name, "TestContract");
    }

    #[test]
    fn rpc_contract_state_roundtrip_json() {
        let nef = NefFile {
            compiler: "neo".into(),
            source: "src".into(),
            tokens: vec![neo_core::smart_contract::method_token::MethodToken::default()],
            script: vec![1, 2, 3],
            checksum: 321,
        };
        let manifest = ContractManifest::new("Contract".into());
        let state = RpcContractState {
            contract_state: ContractState {
                id: 5,
                update_counter: 6,
                hash: UInt160::zero(),
                nef,
                manifest,
            },
        };

        let json = state.to_json().expect("to_json");
        let parsed = RpcContractState::from_json(&json).expect("from_json");
        assert_eq!(parsed.contract_state.id, 5);
        assert_eq!(parsed.contract_state.update_counter, 6);
        assert_eq!(parsed.contract_state.nef.checksum, 321);
        assert_eq!(parsed.contract_state.manifest.name, "Contract");
    }
}

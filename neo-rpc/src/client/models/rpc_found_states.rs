// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_found_states.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use base64::{Engine as _, engine::general_purpose};
use neo_json::JObject;
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
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let truncated = json
            .get("truncated")
            .map(neo_json::JToken::as_boolean)
            .ok_or("Missing or invalid 'truncated' field")?;

        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| {
                        let key = obj
                            .get("key")
                            .and_then(neo_json::JToken::as_string)
                            .and_then(|s| general_purpose::STANDARD.decode(s).ok())?;
                        let value = obj
                            .get("value")
                            .and_then(neo_json::JToken::as_string)
                            .and_then(|s| general_purpose::STANDARD.decode(s).ok())?;
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let first_proof = json
            .get("firstProof")
            .and_then(neo_json::JToken::as_string)
            .and_then(|s| general_purpose::STANDARD.decode(s).ok());

        let last_proof = json
            .get("lastProof")
            .and_then(neo_json::JToken::as_string)
            .and_then(|s| general_purpose::STANDARD.decode(s).ok());

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
        json.insert(
            "truncated".to_string(),
            neo_json::JToken::Boolean(self.truncated),
        );

        let results: Vec<neo_json::JToken> = self
            .results
            .iter()
            .map(|(k, v)| {
                let mut entry = JObject::new();
                entry.insert(
                    "key".to_string(),
                    neo_json::JToken::String(general_purpose::STANDARD.encode(k)),
                );
                entry.insert(
                    "value".to_string(),
                    neo_json::JToken::String(general_purpose::STANDARD.encode(v)),
                );
                neo_json::JToken::Object(entry)
            })
            .collect();
        json.insert(
            "results".to_string(),
            neo_json::JToken::Array(neo_json::JArray::from(results)),
        );

        if let Some(first) = &self.first_proof {
            json.insert(
                "firstProof".to_string(),
                neo_json::JToken::String(general_purpose::STANDARD.encode(first)),
            );
        }
        if let Some(last) = &self.last_proof {
            json.insert(
                "lastProof".to_string(),
                neo_json::JToken::String(general_purpose::STANDARD.encode(last)),
            );
        }

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::{JArray, JToken};

    #[test]
    fn rpc_found_states_parses_results_and_proofs() {
        let mut json = JObject::new();
        json.insert("truncated".to_string(), JToken::Boolean(true));

        let mut entry = JObject::new();
        entry.insert(
            "key".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"k")),
        );
        entry.insert(
            "value".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"v")),
        );
        json.insert(
            "results".to_string(),
            JToken::Array(JArray::from(vec![JToken::Object(entry)])),
        );
        json.insert(
            "firstProof".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"first")),
        );
        json.insert(
            "lastProof".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"last")),
        );

        let parsed = RpcFoundStates::from_json(&json).expect("found states");
        assert!(parsed.truncated);
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].0, b"k");
        assert_eq!(parsed.results[0].1, b"v");
        assert_eq!(parsed.first_proof.unwrap(), b"first");
        assert_eq!(parsed.last_proof.unwrap(), b"last");
    }

    #[test]
    fn rpc_found_states_roundtrip() {
        let found = RpcFoundStates {
            truncated: false,
            results: vec![(b"a".to_vec(), b"b".to_vec())],
            first_proof: None,
            last_proof: Some(b"tail".to_vec()),
        };
        let json = found.to_json();
        let parsed = RpcFoundStates::from_json(&json).expect("found states");
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.last_proof.unwrap(), b"tail");
        // Ensure base64 encoding was applied
        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .expect("results array");
        let first = results.iter().next().and_then(|t| t.as_ref()).unwrap();
        let obj = first.as_object().unwrap();
        assert_eq!(
            obj.get("key").and_then(|v| v.as_string()).unwrap(),
            general_purpose::STANDARD.encode(b"a")
        );
    }
}

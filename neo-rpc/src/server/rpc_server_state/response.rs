//! Typed response construction for StateService RPC handlers.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_state_service::StateRoot;
use serde_json::{Map, Value, json};

pub(super) fn state_height_to_json(index: Option<u32>) -> Value {
    let index = index.map_or(Value::Null, |index| json!(index));
    json!({
        "localrootindex": index,
        "validatedrootindex": index})
}

pub(super) fn proof_payload_to_json(payload: String) -> Value {
    Value::String(payload)
}

pub(super) fn base64_state_value_to_json(value: &[u8]) -> Value {
    Value::String(BASE64_STANDARD.encode(value))
}

/// JSON response for `findstates`.
pub(super) struct FindStatesResponse {
    first_proof: Option<String>,
    last_proof: Option<String>,
    truncated: bool,
    results: Vec<FindStatesEntry>,
}

impl FindStatesResponse {
    pub(super) fn new(
        first_proof: Option<String>,
        last_proof: Option<String>,
        truncated: bool,
        results: Vec<(Vec<u8>, Vec<u8>)>,
    ) -> Self {
        Self {
            first_proof,
            last_proof,
            truncated,
            results: results
                .into_iter()
                .map(|(key, value)| FindStatesEntry { key, value })
                .collect(),
        }
    }

    pub(super) fn into_json(self) -> Value {
        let mut response = Map::new();
        if let Some(first_proof) = self.first_proof {
            response.insert("firstProof".to_string(), Value::String(first_proof));
        }
        if let Some(last_proof) = self.last_proof {
            response.insert("lastProof".to_string(), Value::String(last_proof));
        }
        response.insert("truncated".to_string(), Value::Bool(self.truncated));
        response.insert(
            "results".to_string(),
            Value::Array(
                self.results
                    .into_iter()
                    .map(FindStatesEntry::into_json)
                    .collect(),
            ),
        );
        Value::Object(response)
    }
}

struct FindStatesEntry {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl FindStatesEntry {
    fn into_json(self) -> Value {
        json!({
            "key": BASE64_STANDARD.encode(self.key),
            "value": BASE64_STANDARD.encode(self.value),
        })
    }
}

pub(super) fn state_root_to_json(root: &StateRoot) -> Value {
    let mut obj = Map::new();
    obj.insert("version".to_string(), json!(root.version));
    obj.insert("index".to_string(), json!(root.index));
    obj.insert(
        "roothash".to_string(),
        Value::String(root.root_hash.to_string()),
    );
    // C# `StateRoot.ToJson`: `witnesses = [] | [Witness.ToJson()]`, where a
    // witness is `{ invocation: base64, verification: base64 }`. Emitted when
    // the root is signed (an aggregated StateValidators witness).
    let witnesses = match root.witness() {
        None => Vec::new(),
        Some(witness) => {
            let mut w = Map::new();
            w.insert(
                "invocation".to_string(),
                Value::String(BASE64_STANDARD.encode(&witness.invocation_script)),
            );
            w.insert(
                "verification".to_string(),
                Value::String(BASE64_STANDARD.encode(&witness.verification_script)),
            );
            vec![Value::Object(w)]
        }
    };
    obj.insert("witnesses".to_string(), Value::Array(witnesses));
    Value::Object(obj)
}

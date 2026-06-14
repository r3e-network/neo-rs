use super::super::utility::{base64_string_token, object_array, optional_base64_field_lossy};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
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
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let truncated = json
            .get("truncated")
            .map(neo_serialization::json::JToken::as_boolean)
            .ok_or_else(|| CoreError::other("Missing or invalid 'truncated' field"))?;

        let results = json
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| {
                        let key = optional_base64_field_lossy(obj, "key")?;
                        let value = optional_base64_field_lossy(obj, "value")?;
                        Some((key, value))
                    })
                    .collect()
            })
            .unwrap_or_default();

        let first_proof = optional_base64_field_lossy(json, "firstProof");
        let last_proof = optional_base64_field_lossy(json, "lastProof");

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
        json.insert("truncated".to_string(), JToken::Boolean(self.truncated));

        json.insert(
            "results".to_string(),
            object_array(&self.results, |(key, value)| {
                let mut entry = JObject::new();
                entry.insert("key".to_string(), base64_string_token(key));
                entry.insert("value".to_string(), base64_string_token(value));
                entry
            }),
        );

        if let Some(first) = &self.first_proof {
            json.insert("firstProof".to_string(), base64_string_token(first));
        }
        if let Some(last) = &self.last_proof {
            json.insert("lastProof".to_string(), base64_string_token(last));
        }

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose};
    use neo_serialization::json::{JArray, JToken};

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

    #[test]
    fn rpc_found_states_keeps_lossy_base64_parse_behavior() {
        let mut valid = JObject::new();
        valid.insert(
            "key".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"k")),
        );
        valid.insert(
            "value".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"v")),
        );

        let mut invalid_key = JObject::new();
        invalid_key.insert("key".to_string(), JToken::String("not base64".to_string()));
        invalid_key.insert(
            "value".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"v")),
        );

        let mut invalid_value = JObject::new();
        invalid_value.insert(
            "key".to_string(),
            JToken::String(general_purpose::STANDARD.encode(b"k")),
        );
        invalid_value.insert(
            "value".to_string(),
            JToken::String("not base64".to_string()),
        );

        let mut results = JArray::new();
        results.add(Some(JToken::Object(valid)));
        results.add(Some(JToken::Object(invalid_key)));
        results.add(Some(JToken::Object(invalid_value)));
        results.add(Some(JToken::String("not an object".to_string())));
        results.add(None);

        let mut json = JObject::new();
        json.insert("truncated".to_string(), JToken::Boolean(false));
        json.insert("results".to_string(), JToken::Array(results));
        json.insert(
            "firstProof".to_string(),
            JToken::String("not base64".to_string()),
        );
        json.insert("lastProof".to_string(), JToken::Number(1.0));

        let parsed = RpcFoundStates::from_json(&json).expect("lossy found states");
        assert_eq!(parsed.results, vec![(b"k".to_vec(), b"v".to_vec())]);
        assert_eq!(parsed.first_proof, None);
        assert_eq!(parsed.last_proof, None);
    }

    #[test]
    fn rpc_found_states_keeps_truncated_truthy_coercion() {
        let mut truthy = JObject::new();
        truthy.insert("truncated".to_string(), JToken::String("yes".to_string()));
        assert!(
            RpcFoundStates::from_json(&truthy)
                .expect("truthy truncated")
                .truncated
        );

        let mut falsy = JObject::new();
        falsy.insert("truncated".to_string(), JToken::Number(0.0));
        assert!(
            !RpcFoundStates::from_json(&falsy)
                .expect("falsy truncated")
                .truncated
        );
    }
}

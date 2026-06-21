use super::RpcNefFile;
use neo_error::{CoreError, CoreResult};
use neo_execution::ContractState;
use neo_manifest::ContractManifest;
use neo_primitives::UInt160;
use neo_serialization::json::{JObject, JToken};
use serde_json::{Number as JsonNumber, Value as JsonValue};

/// RPC contract state information matching C# `RpcContractState`
#[derive(Debug, Clone)]
pub struct RpcContractState {
    /// The contract state
    pub contract_state: ContractState,
}

impl RpcContractState {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let id = json
            .get("id")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'id' field"))?
            as i32;

        let update_counter = json
            .get("updatecounter")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'updatecounter' field"))?
            as u16;

        let hash = json
            .get("hash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt160::parse(&s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'hash' field"))?;

        let nef_json = json
            .get("nef")
            .and_then(|v| v.as_object())
            .ok_or_else(|| CoreError::other("Missing or invalid 'nef' field"))?;
        let nef = RpcNefFile::from_json(nef_json)?;

        let manifest_json = json
            .get("manifest")
            .and_then(|v| v.as_object())
            .ok_or_else(|| CoreError::other("Missing or invalid 'manifest' field"))?;
        let manifest_value = serde_json::from_str::<JsonValue>(&manifest_json.to_string())
            .map_err(|err| {
                CoreError::other(format!("Invalid manifest: Serialization error: {err}"))
            })?;
        let manifest_str = normalize_numeric_json(manifest_value).to_string();
        let manifest = ContractManifest::from_json(&manifest_str)
            .map_err(|err| CoreError::other(format!("Invalid manifest: {err}")))?;

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
    /// Matches C# `ToJson`
    pub fn to_json(&self) -> CoreResult<JObject> {
        let mut json = JObject::new();
        json.insert(
            "id".to_string(),
            JToken::Number(f64::from(self.contract_state.id)),
        );
        json.insert(
            "updatecounter".to_string(),
            JToken::Number(f64::from(self.contract_state.update_counter)),
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
            .map_err(|err| CoreError::other(err.to_string()))?;
        let manifest_jtoken =
            neo_serialization::json::JToken::parse(&manifest_json_value.to_string(), 128)
                .map_err(|err| CoreError::other(err.to_string()))?;
        json.insert("manifest".to_string(), manifest_jtoken);

        Ok(json)
    }
}

fn normalize_numeric_json(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(entries) => {
            JsonValue::Array(entries.into_iter().map(normalize_numeric_json).collect())
        }
        JsonValue::Object(entries) => JsonValue::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key, normalize_numeric_json(value)))
                .collect(),
        ),
        JsonValue::Number(number) => {
            if let Some(float) = number.as_f64() {
                if float.fract() == 0.0 {
                    if float >= 0.0 && float <= u64::MAX as f64 {
                        if float <= i64::MAX as f64 {
                            return JsonValue::Number(JsonNumber::from(float as i64));
                        }
                        return JsonValue::Number(JsonNumber::from(float as u64));
                    }
                    if float >= i64::MIN as f64 {
                        return JsonValue::Number(JsonNumber::from(float as i64));
                    }
                }
            }
            JsonValue::Number(number)
        }
        other => other,
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_contract_state.rs"]
mod tests;

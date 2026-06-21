use neo_error::{CoreError, CoreResult};
use neo_payloads::Witness;
use neo_primitives::UInt256;
use neo_serialization::json::{JArray, JObject, JToken};
use serde::{Deserialize, Serialize};

/// State root information matching C# `RpcStateRoot`
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
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let version = json
            .get("version")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'version' field"))?
            as u8;

        let index = json
            .get("index")
            .and_then(neo_serialization::json::JToken::as_number)
            .ok_or_else(|| CoreError::other("Missing or invalid 'index' field"))?
            as u32;

        let root_hash = json
            .get("roothash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'roothash' field"))?;

        let witness = json
            .get("witnesses")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.iter().next())
            .and_then(|entry| entry.as_ref())
            .and_then(|token| token.as_object())
            .and_then(|obj| super::super::utility::witness_from_json(obj).ok());

        Ok(Self {
            version,
            index,
            root_hash,
            witness,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "version".to_string(),
            JToken::Number(f64::from(self.version)),
        );
        json.insert("index".to_string(), JToken::Number(f64::from(self.index)));
        json.insert(
            "roothash".to_string(),
            JToken::String(self.root_hash.to_string()),
        );

        if let Some(witness) = &self.witness {
            let witness_json = super::super::utility::witness_to_json(witness);
            json.insert(
                "witnesses".to_string(),
                JToken::Array(JArray::from(vec![JToken::Object(witness_json)])),
            );
        }

        json
    }
}

#[cfg(test)]
#[path = "../../tests/client/models/rpc_state_root.rs"]
mod tests;

use super::super::utility::{parse_number_or_string_token, parse_uint256_array_lossy, token_array};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt256;
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Raw memory pool information matching C# `RpcRawMemPool`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRawMemPool {
    /// Current block height
    pub height: u32,

    /// List of verified transaction hashes
    pub verified: Vec<UInt256>,

    /// List of unverified transaction hashes
    pub unverified: Vec<UInt256>,
}

impl RpcRawMemPool {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("height".to_string(), JToken::Number(f64::from(self.height)));

        json.insert(
            "verified".to_string(),
            token_array(&self.verified, |hash| JToken::String(hash.to_string())),
        );

        json.insert(
            "unverified".to_string(),
            token_array(&self.unverified, |hash| JToken::String(hash.to_string())),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let height_token = json
            .get("height")
            .ok_or_else(|| CoreError::other("Missing or invalid 'height' field"))?;
        let height = parse_number_or_string_token(
            height_token,
            "height",
            "Missing or invalid 'height' field",
            |value| value as u32,
        )
        .map_err(|e| CoreError::other(e.to_string()))?;

        let verified = parse_uint256_array_lossy(json, "verified");
        let unverified = parse_uint256_array_lossy(json, "unverified");

        Ok(Self {
            height,
            verified,
            unverified,
        })
    }
}

#[cfg(test)]
#[path = "../../../tests/client/models/ledger/rpc_raw_mem_pool.rs"]
mod tests;

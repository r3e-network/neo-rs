use super::super::utility::parse_number_or_string_token;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Unclaimed GAS information matching C# `RpcUnclaimedGas`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUnclaimedGas {
    /// Amount of unclaimed GAS
    pub unclaimed: i64,

    /// Address
    pub address: String,
}

impl RpcUnclaimedGas {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "unclaimed".to_string(),
            JToken::String(self.unclaimed.to_string()),
        );
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let unclaimed_token = json
            .get("unclaimed")
            .ok_or_else(|| CoreError::other("Missing or invalid 'unclaimed' field"))?;
        let unclaimed = parse_number_or_string_token(
            unclaimed_token,
            "unclaimed",
            "Invalid 'unclaimed' field",
            |value| value as i64,
        )
        .map_err(|e| CoreError::other(e.to_string()))?;

        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        Ok(Self { unclaimed, address })
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;

    #[test]
    fn rpc_unclaimed_gas_roundtrip() {
        let gas = RpcUnclaimedGas {
            unclaimed: 1234,
            address: "NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".to_string(),
        };
        let json = gas.to_json();
        let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
        assert_eq!(parsed.unclaimed, gas.unclaimed);
        assert_eq!(parsed.address, gas.address);
    }

    #[test]
    fn rpc_unclaimed_gas_rejects_invalid_value() {
        let mut json = JObject::new();
        json.insert(
            "unclaimed".to_string(),
            JToken::String("not-a-number".into()),
        );
        json.insert(
            "address".to_string(),
            JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
        );
        assert!(RpcUnclaimedGas::from_json(&json).is_err());
    }

    #[test]
    fn rpc_unclaimed_gas_accepts_numeric_value() {
        let mut json = JObject::new();
        json.insert("unclaimed".to_string(), JToken::Number(5f64));
        json.insert(
            "address".to_string(),
            JToken::String("NQ7cbaBqX1p5quJDQr6b1qnBZBHae3mJzA".into()),
        );
        let parsed = RpcUnclaimedGas::from_json(&json).expect("unclaimed");
        assert_eq!(parsed.unclaimed, 5);
    }

    #[test]
    fn unclaimed_gas_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getunclaimedgasasync") else {
            return;
        };
        let parsed = RpcUnclaimedGas::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

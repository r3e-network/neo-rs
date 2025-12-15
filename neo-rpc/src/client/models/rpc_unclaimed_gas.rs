// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_unclaimed_gas.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Unclaimed GAS information matching C# RpcUnclaimedGas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcUnclaimedGas {
    /// Amount of unclaimed GAS
    pub unclaimed: i64,

    /// Address
    pub address: String,
}

impl RpcUnclaimedGas {
    /// Converts to JSON
    /// Matches C# ToJson
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let unclaimed_token = json
            .get("unclaimed")
            .ok_or("Missing or invalid 'unclaimed' field")?;
        let unclaimed = if let Some(text) = unclaimed_token.as_string() {
            text.parse::<i64>()
                .map_err(|_| format!("Invalid unclaimed value: {}", text))?
        } else if let Some(num) = unclaimed_token.as_number() {
            num as i64
        } else {
            return Err("Invalid 'unclaimed' field".to_string());
        };

        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?
            .to_string();

        Ok(Self { unclaimed, address })
    }
}

#[cfg(test)]
mod tests {
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
}

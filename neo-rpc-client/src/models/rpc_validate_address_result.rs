// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_validate_address_result.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Address validation result matching C# RpcValidateAddressResult
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcValidateAddressResult {
    /// The address that was validated
    pub address: String,

    /// Whether the address is valid
    pub is_valid: bool,
}

impl RpcValidateAddressResult {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("isvalid".to_string(), JToken::Boolean(self.is_valid));
        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?
            .to_string();

        let is_valid = json
            .get("isvalid")
            .map(|v| v.as_boolean())
            .ok_or("Missing or invalid 'isvalid' field")?;

        Ok(Self { address, is_valid })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_address_roundtrip() {
        let result = RpcValidateAddressResult {
            address: "addr".to_string(),
            is_valid: true,
        };
        let json = result.to_json();
        let parsed = RpcValidateAddressResult::from_json(&json).unwrap();
        assert_eq!(parsed.address, result.address);
        assert!(parsed.is_valid);
    }
}

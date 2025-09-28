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
        json.insert("unclaimed".to_string(), JToken::String(self.unclaimed.to_string()));
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let unclaimed_str = json.get("unclaimed")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'unclaimed' field")?;
        let unclaimed = unclaimed_str.parse::<i64>()
            .map_err(|_| format!("Invalid unclaimed value: {}", unclaimed_str))?;
            
        let address = json.get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?
            .to_string();
            
        Ok(Self {
            unclaimed,
            address,
        })
    }
}
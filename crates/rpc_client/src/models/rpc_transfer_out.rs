// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_transfer_out.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, UInt160};
use neo_json::{JObject, JToken};
use serde::{Deserialize, Serialize};

/// Transfer output information matching C# RpcTransferOut
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransferOut {
    /// Asset hash
    pub asset: UInt160,
    
    /// Script hash of recipient
    pub script_hash: UInt160,
    
    /// Transfer value
    pub value: String,
}

impl RpcTransferOut {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        json.insert("asset".to_string(), JToken::String(self.asset.to_string()));
        json.insert("value".to_string(), JToken::String(self.value.clone()));
        json.insert("address".to_string(), JToken::String(
            self.script_hash.to_address(protocol_settings.address_version)
        ));
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let asset_str = json.get("asset")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'asset' field")?;
            
        let asset = if asset_str.starts_with("0x") {
            UInt160::parse(asset_str)
        } else {
            UInt160::from_address(asset_str, protocol_settings.address_version)
        }.map_err(|_| format!("Invalid asset: {}", asset_str))?;
        
        let value = json.get("value")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'value' field")?
            .to_string();
            
        let address = json.get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?;
            
        let script_hash = UInt160::from_address(address, protocol_settings.address_version)
            .map_err(|_| format!("Invalid address: {}", address))?;
            
        Ok(Self {
            asset,
            script_hash,
            value,
        })
    }
}
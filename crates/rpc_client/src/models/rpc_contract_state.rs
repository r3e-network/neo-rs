// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_contract_state.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::RpcNefFile;
use neo_core::{ContractManifest, ContractState, UInt160};
use neo_json::JObject;
use serde::{Deserialize, Serialize};

/// RPC contract state information matching C# RpcContractState
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcContractState {
    /// The contract state
    pub contract_state: ContractState,
}

impl RpcContractState {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        self.contract_state.to_json()
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let id = json.get("id")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'id' field")?
            as i32;
            
        let update_counter = json.get("updatecounter")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'updatecounter' field")?
            as u16;
            
        let hash = json.get("hash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(s).ok())
            .ok_or("Missing or invalid 'hash' field")?;
            
        let nef_json = json.get("nef")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'nef' field")?;
        let nef = RpcNefFile::from_json(nef_json)?;
        
        let manifest_json = json.get("manifest")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'manifest' field")?;
        let manifest = ContractManifest::from_json(manifest_json)?;
        
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
}
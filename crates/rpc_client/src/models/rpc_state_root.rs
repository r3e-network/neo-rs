// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_state_root.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{UInt256, Witness};
use neo_json::JObject;
use serde::{Deserialize, Serialize};

/// State root information matching C# RpcStateRoot
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let version = json.get("version")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'version' field")?
            as u8;
            
        let index = json.get("index")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'index' field")?
            as u32;
            
        let root_hash = json.get("roothash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(s).ok())
            .ok_or("Missing or invalid 'roothash' field")?;
            
        let witness = json.get("witnesses")
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|w| w.as_object())
            .and_then(|obj| crate::utility::witness_from_json(obj).ok());
            
        Ok(Self {
            version,
            index,
            root_hash,
            witness,
        })
    }
}
// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_transaction.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, Transaction, UInt256};
use neo_json::JObject;
use neo_vm::VMState;
use serde::{Deserialize, Serialize};

/// RPC transaction information matching C# RpcTransaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcTransaction {
    /// The transaction
    pub transaction: Transaction,
    
    /// Block hash if confirmed
    pub block_hash: Option<UInt256>,
    
    /// Number of confirmations
    pub confirmations: Option<u32>,
    
    /// Block timestamp
    pub block_time: Option<u64>,
    
    /// VM execution state
    pub vm_state: Option<VMState>,
}

impl RpcTransaction {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = crate::utility::transaction_to_json(&self.transaction, protocol_settings);
        
        if let Some(confirmations) = self.confirmations {
            if let Some(ref block_hash) = self.block_hash {
                json.insert("blockhash".to_string(), neo_json::JToken::String(block_hash.to_string()));
            }
            json.insert("confirmations".to_string(), neo_json::JToken::Number(confirmations as f64));
            
            if let Some(block_time) = self.block_time {
                json.insert("blocktime".to_string(), neo_json::JToken::Number(block_time as f64));
            }
            
            if let Some(ref vm_state) = self.vm_state {
                json.insert("vmstate".to_string(), neo_json::JToken::String(vm_state.to_string()));
            }
        }
        
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let transaction = crate::utility::transaction_from_json(json, protocol_settings)?;
        
        let (block_hash, confirmations, block_time, vm_state) = 
            if json.get("confirmations").is_some() {
                let block_hash = json.get("blockhash")
                    .and_then(|v| v.as_string())
                    .and_then(|s| UInt256::parse(s).ok());
                    
                let confirmations = json.get("confirmations")
                    .and_then(|v| v.as_number())
                    .map(|n| n as u32);
                    
                let block_time = json.get("blocktime")
                    .and_then(|v| v.as_number())
                    .map(|n| n as u64);
                    
                let vm_state = json.get("vmstate")
                    .and_then(|v| v.as_string())
                    .and_then(|s| VMState::from_str(s).ok());
                    
                (block_hash, confirmations, block_time, vm_state)
            } else {
                (None, None, None, None)
            };
            
        Ok(Self {
            transaction,
            block_hash,
            confirmations,
            block_time,
            vm_state,
        })
    }
}
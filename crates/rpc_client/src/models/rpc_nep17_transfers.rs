// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_nep17_transfers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, UInt160, UInt256};
use neo_json::{JArray, JObject, JToken};
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// NEP17 transfers for an address matching C# RpcNep17Transfers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfers {
    /// User script hash
    pub user_script_hash: UInt160,
    
    /// List of sent transfers
    pub sent: Vec<RpcNep17Transfer>,
    
    /// List of received transfers
    pub received: Vec<RpcNep17Transfer>,
}

impl RpcNep17Transfers {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        
        let sent_array: Vec<JToken> = self.sent
            .iter()
            .map(|t| JToken::Object(t.to_json(protocol_settings)))
            .collect();
        json.insert("sent".to_string(), JToken::Array(JArray::from(sent_array)));
        
        let received_array: Vec<JToken> = self.received
            .iter()
            .map(|t| JToken::Object(t.to_json(protocol_settings)))
            .collect();
        json.insert("received".to_string(), JToken::Array(JArray::from(received_array)));
        
        json.insert("address".to_string(), JToken::String(
            self.user_script_hash.to_address(protocol_settings.address_version)
        ));
        
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let sent = json.get("sent")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| RpcNep17Transfer::from_json(obj, protocol_settings).ok())
                    .collect()
            })
            .unwrap_or_default();
            
        let received = json.get("received")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| RpcNep17Transfer::from_json(obj, protocol_settings).ok())
                    .collect()
            })
            .unwrap_or_default();
            
        let address = json.get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?;
            
        let user_script_hash = UInt160::from_address(address, protocol_settings.address_version)
            .map_err(|_| format!("Invalid address: {}", address))?;
            
        Ok(Self {
            user_script_hash,
            sent,
            received,
        })
    }
}

/// Individual NEP17 transfer entry matching C# RpcNep17Transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNep17Transfer {
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    
    /// Asset hash
    pub asset_hash: UInt160,
    
    /// Transfer address script hash
    pub user_script_hash: Option<UInt160>,
    
    /// Transfer amount
    pub amount: BigInt,
    
    /// Block index
    pub block_index: u32,
    
    /// Transfer notify index
    pub transfer_notify_index: u16,
    
    /// Transaction hash
    pub tx_hash: UInt256,
}

impl RpcNep17Transfer {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self, protocol_settings: &ProtocolSettings) -> JObject {
        let mut json = JObject::new();
        json.insert("timestamp".to_string(), JToken::Number(self.timestamp_ms as f64));
        json.insert("assethash".to_string(), JToken::String(self.asset_hash.to_string()));
        
        if let Some(ref user_script_hash) = self.user_script_hash {
            json.insert("transferaddress".to_string(), JToken::String(
                user_script_hash.to_address(protocol_settings.address_version)
            ));
        } else {
            json.insert("transferaddress".to_string(), JToken::Null);
        }
        
        json.insert("amount".to_string(), JToken::String(self.amount.to_string()));
        json.insert("blockindex".to_string(), JToken::Number(self.block_index as f64));
        json.insert("transfernotifyindex".to_string(), JToken::Number(self.transfer_notify_index as f64));
        json.insert("txhash".to_string(), JToken::String(self.tx_hash.to_string()));
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let timestamp_ms = json.get("timestamp")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'timestamp' field")?
            as u64;
            
        let asset_hash_str = json.get("assethash")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'assethash' field")?;
            
        let asset_hash = if asset_hash_str.starts_with("0x") {
            UInt160::parse(asset_hash_str)
        } else {
            UInt160::from_address(asset_hash_str, protocol_settings.address_version)
        }.map_err(|_| format!("Invalid asset hash: {}", asset_hash_str))?;
        
        let user_script_hash = json.get("transferaddress")
            .and_then(|v| v.as_string())
            .and_then(|addr| {
                if addr.starts_with("0x") {
                    UInt160::parse(addr).ok()
                } else {
                    UInt160::from_address(addr, protocol_settings.address_version).ok()
                }
            });
            
        let amount_str = json.get("amount")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'amount' field")?;
        let amount = BigInt::from_str(amount_str)
            .map_err(|_| format!("Invalid amount: {}", amount_str))?;
            
        let block_index = json.get("blockindex")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'blockindex' field")?
            as u32;
            
        let transfer_notify_index = json.get("transfernotifyindex")
            .and_then(|v| v.as_number())
            .ok_or("Missing or invalid 'transfernotifyindex' field")?
            as u16;
            
        let tx_hash = json.get("txhash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(s).ok())
            .ok_or("Missing or invalid 'txhash' field")?;
            
        Ok(Self {
            timestamp_ms,
            asset_hash,
            user_script_hash,
            amount,
            block_index,
            transfer_notify_index,
            tx_hash,
        })
    }
}
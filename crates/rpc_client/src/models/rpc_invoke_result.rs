// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_invoke_result.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use neo_vm::{StackItem, VMState};
use serde::{Deserialize, Serialize};

/// RPC invoke result matching C# RpcInvokeResult
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcInvokeResult {
    /// The script that was invoked
    pub script: String,
    
    /// VM execution state
    pub state: VMState,
    
    /// Gas consumed during execution
    pub gas_consumed: i64,
    
    /// Stack items after execution
    pub stack: Vec<StackItem>,
    
    /// Transaction if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx: Option<String>,
    
    /// Exception message if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception: Option<String>,
    
    /// Session ID if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl RpcInvokeResult {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String(self.script.clone()));
        json.insert("state".to_string(), JToken::String(self.state.to_string()));
        json.insert("gasconsumed".to_string(), JToken::String(self.gas_consumed.to_string()));
        
        if let Some(ref exception) = self.exception {
            json.insert("exception".to_string(), JToken::String(exception.clone()));
        }
        
        // Try to serialize stack items
        match self.stack.iter().map(|item| item.to_json()).collect::<Result<Vec<_>, _>>() {
            Ok(stack_json) => {
                json.insert("stack".to_string(), JToken::Array(JArray::from(stack_json)));
            }
            Err(_) => {
                // Handle recursive reference error
                json.insert("stack".to_string(), JToken::String("error: recursive reference".to_string()));
            }
        }
        
        if let Some(ref tx) = self.tx {
            json.insert("tx".to_string(), JToken::String(tx.clone()));
        }
        
        if let Some(ref session) = self.session {
            json.insert("session".to_string(), JToken::String(session.clone()));
        }
        
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let script = json.get("script")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'script' field")?
            .to_string();
            
        let state_str = json.get("state")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'state' field")?;
        let state = VMState::from_str(state_str)
            .map_err(|_| format!("Invalid VM state: {}", state_str))?;
            
        let gas_consumed_str = json.get("gasconsumed")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'gasconsumed' field")?;
        let gas_consumed = gas_consumed_str.parse::<i64>()
            .map_err(|_| format!("Invalid gas consumed value: {}", gas_consumed_str))?;
            
        let exception = json.get("exception")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());
            
        let session = json.get("session")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());
            
        let tx = json.get("tx")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());
            
        // Try to parse stack items
        let stack = json.get("stack")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| crate::utility::stack_item_from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();
            
        Ok(Self {
            script,
            state,
            gas_consumed,
            stack,
            tx,
            exception,
            session,
        })
    }
}

/// RPC stack item representation matching C# RpcStack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcStack {
    /// Stack item type
    #[serde(rename = "type")]
    pub item_type: String,
    
    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("type".to_string(), JToken::String(self.item_type.clone()));
        json.insert("value".to_string(), self.value.clone());
        json
    }
    
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let item_type = json.get("type")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'type' field")?
            .to_string();
            
        let value = json.get("value")
            .ok_or("Missing 'value' field")?
            .clone();
            
        Ok(Self {
            item_type,
            value,
        })
    }
}
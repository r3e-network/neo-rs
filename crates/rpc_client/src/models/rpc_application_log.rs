// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_application_log.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ProtocolSettings, TriggerType, UInt160, UInt256};
use neo_json::{JArray, JObject, JToken};
use neo_vm::{StackItem, VMState};
use serde::{Deserialize, Serialize};

/// Application log information matching C# RpcApplicationLog
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcApplicationLog {
    /// Transaction ID
    pub tx_id: Option<UInt256>,

    /// Block hash
    pub block_hash: Option<UInt256>,

    /// List of executions
    pub executions: Vec<Execution>,
}

impl RpcApplicationLog {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();

        if let Some(ref tx_id) = self.tx_id {
            json.insert("txid".to_string(), JToken::String(tx_id.to_string()));
        }

        if let Some(ref block_hash) = self.block_hash {
            json.insert(
                "blockhash".to_string(),
                JToken::String(block_hash.to_string()),
            );
        }

        let executions_array: Vec<JToken> = self
            .executions
            .iter()
            .map(|e| JToken::Object(e.to_json()))
            .collect();
        json.insert(
            "executions".to_string(),
            JToken::Array(JArray::from(executions_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let tx_id = json
            .get("txid")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(s).ok());

        let block_hash = json
            .get("blockhash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(s).ok());

        let executions = json
            .get("executions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| Execution::from_json(obj, protocol_settings).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            tx_id,
            block_hash,
            executions,
        })
    }
}

/// Execution information matching C# Execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    /// Trigger type
    pub trigger: TriggerType,

    /// VM state
    pub vm_state: VMState,

    /// Gas consumed
    pub gas_consumed: i64,

    /// Exception message if any
    pub exception_message: Option<String>,

    /// Stack items
    pub stack: Vec<StackItem>,

    /// Notifications
    pub notifications: Vec<RpcNotifyEventArgs>,
}

impl Execution {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "trigger".to_string(),
            JToken::String(self.trigger.to_string()),
        );
        json.insert(
            "vmstate".to_string(),
            JToken::String(self.vm_state.to_string()),
        );
        json.insert(
            "gasconsumed".to_string(),
            JToken::String(self.gas_consumed.to_string()),
        );

        if let Some(ref exception) = self.exception_message {
            json.insert("exception".to_string(), JToken::String(exception.clone()));
        }

        let stack_array: Vec<JToken> = self
            .stack
            .iter()
            .filter_map(|s| s.to_json().ok())
            .map(JToken::Object)
            .collect();
        json.insert(
            "stack".to_string(),
            JToken::Array(JArray::from(stack_array)),
        );

        let notifications_array: Vec<JToken> = self
            .notifications
            .iter()
            .map(|n| JToken::Object(n.to_json()))
            .collect();
        json.insert(
            "notifications".to_string(),
            JToken::Array(JArray::from(notifications_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let trigger_str = json
            .get("trigger")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'trigger' field")?;
        let trigger = TriggerType::from_str(trigger_str)
            .map_err(|_| format!("Invalid trigger type: {}", trigger_str))?;

        let vm_state_str = json
            .get("vmstate")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'vmstate' field")?;
        let vm_state = VMState::from_str(vm_state_str)
            .map_err(|_| format!("Invalid VM state: {}", vm_state_str))?;

        let gas_consumed_str = json
            .get("gasconsumed")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'gasconsumed' field")?;
        let gas_consumed = gas_consumed_str
            .parse::<i64>()
            .map_err(|_| format!("Invalid gas consumed value: {}", gas_consumed_str))?;

        let exception_message = json
            .get("exception")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let stack = json
            .get("stack")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| crate::utility::stack_item_from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        let notifications = json
            .get("notifications")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_object())
                    .filter_map(|obj| RpcNotifyEventArgs::from_json(obj, protocol_settings).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            trigger,
            vm_state,
            gas_consumed,
            exception_message,
            stack,
            notifications,
        })
    }
}

/// Notification event arguments matching C# RpcNotifyEventArgs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNotifyEventArgs {
    /// Contract that emitted the notification
    pub contract: UInt160,

    /// Event name
    pub event_name: String,

    /// Event state/data
    pub state: StackItem,
}

impl RpcNotifyEventArgs {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "contract".to_string(),
            JToken::String(self.contract.to_string()),
        );
        json.insert(
            "eventname".to_string(),
            JToken::String(self.event_name.clone()),
        );

        if let Ok(state_json) = self.state.to_json() {
            json.insert("state".to_string(), JToken::Object(state_json));
        }

        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        let contract = json
            .get("contract")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(s).ok())
            .ok_or("Missing or invalid 'contract' field")?;

        let event_name = json
            .get("eventname")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'eventname' field")?
            .to_string();

        let state_json = json
            .get("state")
            .and_then(|v| v.as_object())
            .ok_or("Missing or invalid 'state' field")?;
        let state = crate::utility::stack_item_from_json(state_json)?;

        Ok(Self {
            contract,
            event_name,
            state,
        })
    }
}

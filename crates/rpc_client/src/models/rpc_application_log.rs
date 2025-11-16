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

use super::vm_state_utils::vm_state_from_str;
use neo_core::smart_contract::TriggerType;
use neo_core::{ProtocolSettings, UInt160, UInt256};
use neo_json::JObject;
use neo_vm::{StackItem, VMState};
use std::str::FromStr;

/// Application log information matching C# RpcApplicationLog
#[derive(Debug, Clone)]
pub struct RpcApplicationLog {
    /// Transaction ID
    pub tx_id: Option<UInt256>,

    /// Block hash
    pub block_hash: Option<UInt256>,

    /// List of executions
    pub executions: Vec<Execution>,
}

impl RpcApplicationLog {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> Result<Self, String> {
        let tx_id = json
            .get("txid")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(&s).ok());

        let block_hash = json
            .get("blockhash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(&s).ok());

        let executions = json
            .get("executions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
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
#[derive(Debug, Clone)]
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
        let vm_state = vm_state_from_str(&vm_state_str)
            .ok_or_else(|| format!("Invalid VM state: {}", vm_state_str))?;

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
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| crate::utility::stack_item_from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        let notifications = json
            .get("notifications")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
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
#[derive(Debug, Clone)]
pub struct RpcNotifyEventArgs {
    /// Contract that emitted the notification
    pub contract: UInt160,

    /// Event name
    pub event_name: String,

    /// Event state/data
    pub state: StackItem,
}

impl RpcNotifyEventArgs {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> Result<Self, String> {
        let contract = json
            .get("contract")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt160::parse(&s).ok())
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

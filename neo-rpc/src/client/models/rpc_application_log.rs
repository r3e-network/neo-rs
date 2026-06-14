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

use super::super::utility::{
    empty_array, insert_optional_string, object_array, parse_object_array_lossy, required_string,
    stack_item_from_json, stack_item_to_json, stack_items_from_json_field, stack_items_to_json,
};
use super::vm_state_utils::{
    insert_gas_consumed_field, insert_vm_state_field, parse_gas_consumed_field,
    parse_vm_state_field,
};
use std::str::FromStr;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_primitives::TriggerType;
use neo_primitives::{UInt160, UInt256};
use neo_serialization::json::{JObject, JToken};
use neo_vm_rs::StackValue;
use neo_vm_rs::VmState;
/// Application log information matching C# `RpcApplicationLog`
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
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let tx_id = json
            .get("txid")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok());

        let block_hash = json
            .get("blockhash")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt256::parse(&s).ok());

        let executions = parse_object_array_lossy(json, "executions", |obj| {
            Execution::from_json(obj, protocol_settings).map_err(|e| e.to_string())
        });

        Ok(Self {
            tx_id,
            block_hash,
            executions,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        if let Some(tx_id) = &self.tx_id {
            json.insert("txid".to_string(), JToken::String(tx_id.to_string()));
        }
        if let Some(block_hash) = &self.block_hash {
            json.insert(
                "blockhash".to_string(),
                JToken::String(block_hash.to_string()),
            );
        }
        json.insert(
            "executions".to_string(),
            object_array(&self.executions, Execution::to_json),
        );
        json
    }
}

/// Execution information matching C# Execution
#[derive(Debug, Clone)]
pub struct Execution {
    /// Trigger type
    pub trigger: TriggerType,

    /// VM state
    pub vm_state: VmState,

    /// Gas consumed
    pub gas_consumed: i64,

    /// Exception message if any
    pub exception_message: Option<String>,

    /// Stack items
    pub stack: Vec<StackValue>,

    /// Notifications
    pub notifications: Vec<RpcNotifyEventArgs>,
}

impl Execution {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject, protocol_settings: &ProtocolSettings) -> CoreResult<Self> {
        let trigger_str = required_string(json, "trigger")
            .map_err(|e| CoreError::other(e.to_string()))?;
        let trigger = TriggerType::from_str(&trigger_str)
            .map_err(|_| CoreError::other(format!("Invalid trigger type: {trigger_str}")))?;

        let vm_state = parse_vm_state_field(json, "vmstate")?;
        let gas_consumed = parse_gas_consumed_field(json)?;

        let exception_message = json
            .get("exception")
            .and_then(neo_serialization::json::JToken::as_string);

        let stack = stack_items_from_json_field(json, "stack");

        let notifications = parse_object_array_lossy(json, "notifications", |obj| {
            RpcNotifyEventArgs::from_json(obj, protocol_settings).map_err(|e| e.to_string())
        });

        Ok(Self {
            trigger,
            vm_state,
            gas_consumed,
            exception_message,
            stack,
            notifications,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "trigger".to_string(),
            JToken::String(trigger_type_to_string(self.trigger)),
        );
        insert_vm_state_field(&mut json, "vmstate", self.vm_state);
        insert_gas_consumed_field(&mut json, self.gas_consumed);
        insert_optional_string(&mut json, "exception", self.exception_message.as_deref());
        json.insert(
            "stack".to_string(),
            stack_items_to_json(&self.stack).unwrap_or_else(|_| empty_array()),
        );
        json.insert(
            "notifications".to_string(),
            object_array(&self.notifications, RpcNotifyEventArgs::to_json),
        );
        json
    }
}

/// Notification event arguments matching C# `RpcNotifyEventArgs`
#[derive(Debug, Clone)]
pub struct RpcNotifyEventArgs {
    /// Contract that emitted the notification
    pub contract: UInt160,

    /// Event name
    pub event_name: String,

    /// Event state/data
    pub state: StackValue,
}

impl RpcNotifyEventArgs {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(
        json: &JObject,
        _protocol_settings: &ProtocolSettings,
    ) -> CoreResult<Self> {
        let contract = json
            .get("contract")
            .and_then(neo_serialization::json::JToken::as_string)
            .and_then(|s| UInt160::parse(&s).ok())
            .ok_or_else(|| CoreError::other("Missing or invalid 'contract' field"))?;

        let event_name = required_string(json, "eventname")
            .map_err(|e| CoreError::other(e.to_string()))?;

        let state_json = json
            .get("state")
            .and_then(|v| v.as_object())
            .ok_or_else(|| CoreError::other("Missing or invalid 'state' field"))?;
        let state = stack_item_from_json(state_json)
            .map_err(|e| CoreError::other(e.to_string()))?;

        Ok(Self {
            contract,
            event_name,
            state,
        })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
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
        json.insert(
            "state".to_string(),
            JToken::Object(stack_item_to_json(&self.state).unwrap_or_else(|_| JObject::new())),
        );
        json
    }
}

fn trigger_type_to_string(trigger: TriggerType) -> String {
    if trigger == TriggerType::ON_PERSIST {
        "OnPersist".to_string()
    } else if trigger == TriggerType::POST_PERSIST {
        "PostPersist".to_string()
    } else if trigger == TriggerType::VERIFICATION {
        "Verification".to_string()
    } else if trigger == TriggerType::APPLICATION {
        "Application".to_string()
    } else if trigger == TriggerType::SYSTEM {
        "System".to_string()
    } else if trigger == TriggerType::ALL {
        "All".to_string()
    } else {
        format!("{trigger:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_fixtures::rpc_case_result;
    use super::*;
    use neo_serialization::json::{JArray, JToken};

    fn sample_stack_item() -> JObject {
        let mut item = JObject::new();
        item.insert("type".to_string(), JToken::String("Boolean".to_string()));
        item.insert("value".to_string(), JToken::Boolean(true));
        item
    }

    #[test]
    fn parses_application_log() {
        let mut execution = JObject::new();
        execution.insert(
            "trigger".to_string(),
            JToken::String("OnPersist".to_string()),
        );
        execution.insert("vmstate".to_string(), JToken::String("HALT".to_string()));
        execution.insert("gasconsumed".to_string(), JToken::String("1".to_string()));
        execution.insert("exception".to_string(), JToken::Null);

        let mut stack_array = JArray::new();
        stack_array.add(Some(JToken::Object(sample_stack_item())));
        execution.insert("stack".to_string(), JToken::Array(stack_array));

        let mut notification = JObject::new();
        notification.insert(
            "contract".to_string(),
            JToken::String("0000000000000000000000000000000000000000".to_string()),
        );
        notification.insert(
            "eventname".to_string(),
            JToken::String("TestEvent".to_string()),
        );
        notification.insert("state".to_string(), JToken::Object(sample_stack_item()));
        let mut notifications = JArray::new();
        notifications.add(Some(JToken::Object(notification)));
        execution.insert("notifications".to_string(), JToken::Array(notifications));

        let mut executions = JArray::new();
        executions.add(Some(JToken::Object(execution)));

        let mut root = JObject::new();
        root.insert("executions".to_string(), JToken::Array(executions));

        let parsed =
            RpcApplicationLog::from_json(&root, &ProtocolSettings::default_settings()).unwrap();
        assert_eq!(parsed.executions.len(), 1);
        let exec = &parsed.executions[0];
        assert_eq!(exec.trigger, TriggerType::ON_PERSIST);
        assert_eq!(exec.vm_state, VmState::Halt);
        assert_eq!(exec.gas_consumed, 1);
        assert_eq!(exec.stack.len(), 1);
        assert_eq!(exec.notifications.len(), 1);
        assert_eq!(exec.notifications[0].event_name, "TestEvent");
    }

    #[test]
    fn application_log_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getapplicationlogasync") else {
            return;
        };
        let parsed = RpcApplicationLog::from_json(&expected, &ProtocolSettings::default_settings())
            .expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }

    #[test]
    fn application_log_trigger_filter_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("getapplicationlogasync_triggertype") else {
            return;
        };
        let parsed = RpcApplicationLog::from_json(&expected, &ProtocolSettings::default_settings())
            .expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

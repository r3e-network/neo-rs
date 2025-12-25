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

use super::vm_state_utils::{vm_state_from_str, vm_state_to_string};
use std::str::FromStr;

use neo_config::ProtocolSettings;
use neo_core::smart_contract::TriggerType;
use neo_json::{JArray, JObject, JToken};
use neo_primitives::{UInt160, UInt256};
use neo_vm::{StackItem, VMState};
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

    /// Converts to JSON
    /// Matches C# ToJson
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
        let executions = self
            .executions
            .iter()
            .map(|exec| JToken::Object(exec.to_json()))
            .collect::<Vec<_>>();
        json.insert("executions".to_string(), JToken::Array(JArray::from(executions)));
        json
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
        let trigger = TriggerType::from_str(&trigger_str)
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
                    .filter_map(|obj| super::super::utility::stack_item_from_json(obj).ok())
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

    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert(
            "trigger".to_string(),
            JToken::String(trigger_type_to_string(self.trigger)),
        );
        json.insert(
            "vmstate".to_string(),
            JToken::String(vm_state_to_string(self.vm_state)),
        );
        json.insert(
            "gasconsumed".to_string(),
            JToken::String(self.gas_consumed.to_string()),
        );
        json.insert(
            "exception".to_string(),
            self.exception_message
                .as_ref()
                .map(|value| JToken::String(value.clone()))
                .unwrap_or(JToken::Null),
        );
        let stack = self
            .stack
            .iter()
            .map(super::super::utility::stack_item_to_json)
            .collect::<Result<Vec<_>, _>>()
            .unwrap_or_default()
            .into_iter()
            .map(JToken::Object)
            .collect::<Vec<_>>();
        json.insert("stack".to_string(), JToken::Array(JArray::from(stack)));
        let notifications = self
            .notifications
            .iter()
            .map(|notice| JToken::Object(notice.to_json()))
            .collect::<Vec<_>>();
        json.insert(
            "notifications".to_string(),
            JToken::Array(JArray::from(notifications)),
        );
        json
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
        let state = super::super::utility::stack_item_from_json(state_json)?;

        Ok(Self {
            contract,
            event_name,
            state,
        })
    }

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
        json.insert(
            "state".to_string(),
            JToken::Object(
                super::super::utility::stack_item_to_json(&self.state)
                    .unwrap_or_else(|_| JObject::new()),
            ),
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
        format!("{:?}", trigger)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::{JArray, JToken};
    use std::fs;
    use std::path::PathBuf;

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
        assert_eq!(exec.vm_state, VMState::HALT);
        assert_eq!(exec.gas_consumed, 1);
        assert_eq!(exec.stack.len(), 1);
        assert_eq!(exec.notifications.len(), 1);
        assert_eq!(exec.notifications[0].event_name, "TestEvent");
    }

    fn load_rpc_case_result(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("tests");
        path.push("Neo.RpcClient.Tests");
        path.push("RpcTestCases.json");
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token.as_array().expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return result.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
    }

    #[test]
    fn application_log_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("getapplicationlogasync");
        let parsed =
            RpcApplicationLog::from_json(&expected, &ProtocolSettings::default_settings())
                .expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }

    #[test]
    fn application_log_trigger_filter_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("getapplicationlogasync_triggertype");
        let parsed =
            RpcApplicationLog::from_json(&expected, &ProtocolSettings::default_settings())
                .expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

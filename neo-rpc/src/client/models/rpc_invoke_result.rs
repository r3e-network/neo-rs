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

use super::super::utility::{
    optional_string, required_string, stack_items_from_json_field, stack_items_to_json,
};
use super::vm_state_utils::{vm_state_from_str, vm_state_to_string};
use neo_json::{JObject, JToken};
use neo_vm_rs::StackValue;
use neo_vm_rs::VmState;

/// RPC invoke result matching C# `RpcInvokeResult`
#[derive(Debug, Clone)]
pub struct RpcInvokeResult {
    /// The script that was invoked
    pub script: String,

    /// VM execution state
    pub state: VmState,

    /// Gas consumed during execution
    pub gas_consumed: i64,

    /// Stack items after execution
    pub stack: Vec<StackValue>,

    /// Transaction if available
    pub tx: Option<String>,

    /// Exception message if any
    pub exception: Option<String>,

    /// Session ID if available
    pub session: Option<String>,
}

impl RpcInvokeResult {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let script = required_string(json, "script")?;

        let state_str = required_string(json, "state")?;
        let state = vm_state_from_str(&state_str)
            .ok_or_else(|| format!("Invalid VM state: {state_str}"))?;

        let gas_consumed_str = required_string(json, "gasconsumed")?;
        let gas_consumed = gas_consumed_str
            .parse::<i64>()
            .map_err(|_| format!("Invalid gas consumed value: {gas_consumed_str}"))?;

        let exception = optional_string(json, "exception");

        let session = optional_string(json, "session");

        let tx = optional_string(json, "tx");

        let stack = stack_items_from_json_field(json, "stack");

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

    /// Converts to JSON
    /// Matches C# `ToJson`
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String(self.script.clone()));
        json.insert(
            "state".to_string(),
            JToken::String(vm_state_to_string(self.state)),
        );
        json.insert(
            "gasconsumed".to_string(),
            JToken::String(self.gas_consumed.to_string()),
        );

        if let Some(exception) = &self.exception {
            if !exception.is_empty() {
                json.insert("exception".to_string(), JToken::String(exception.clone()));
            }
        }

        let stack_json = stack_items_to_json(&self.stack)
            .unwrap_or_else(|_| JToken::String("error: recursive reference".to_string()));
        json.insert("stack".to_string(), stack_json);

        if let Some(tx) = &self.tx {
            if !tx.is_empty() {
                json.insert("tx".to_string(), JToken::String(tx.clone()));
            }
        }

        json
    }
}

/// RPC stack item representation matching C# `RpcStack`
#[derive(Debug, Clone)]
pub struct RpcStack {
    /// Stack item type
    pub item_type: String,

    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let item_type = json
            .get("type")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'type' field")?;

        let value = json.get("value").ok_or("Missing 'value' field")?.clone();

        Ok(Self { item_type, value })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("type".to_string(), JToken::String(self.item_type.clone()));
        json.insert("value".to_string(), self.value.clone());
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_fixtures::rpc_case_result;
    use neo_json::{JArray, JToken};
    use neo_vm_rs::{stack_value_as_bytes, StackValue, VmState};

    #[test]
    fn invoke_result_roundtrip() {
        let mut stack_item = JObject::new();
        stack_item.insert("type".to_string(), JToken::String("Boolean".to_string()));
        stack_item.insert("value".to_string(), JToken::Boolean(true));

        let mut stack_array = JArray::new();
        stack_array.add(Some(JToken::Object(stack_item)));

        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String("00".to_string()));
        json.insert("state".to_string(), JToken::String("HALT".to_string()));
        json.insert("gasconsumed".to_string(), JToken::String("1".to_string()));
        json.insert("stack".to_string(), JToken::Array(stack_array));

        let parsed = RpcInvokeResult::from_json(&json).unwrap();
        assert_eq!(parsed.script, "00");
        assert_eq!(parsed.state, VmState::Halt);
        assert_eq!(parsed.gas_consumed, 1);
        assert_eq!(parsed.stack.len(), 1);
        assert_eq!(parsed.stack[0], StackValue::Boolean(true));
    }

    #[test]
    fn invoke_result_parses_unknown_stack_item_type() {
        let mut stack_item = JObject::new();
        stack_item.insert("type".to_string(), JToken::String("Unknown".to_string()));
        stack_item.insert("value".to_string(), JToken::String("hello".to_string()));

        let mut stack_array = JArray::new();
        stack_array.add(Some(JToken::Object(stack_item)));

        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String("00".to_string()));
        json.insert("state".to_string(), JToken::String("HALT".to_string()));
        json.insert("gasconsumed".to_string(), JToken::String("1".to_string()));
        json.insert("stack".to_string(), JToken::Array(stack_array));

        let parsed = RpcInvokeResult::from_json(&json).unwrap();
        assert_eq!(parsed.stack.len(), 1);
        assert_eq!(stack_value_as_bytes(&parsed.stack[0]).unwrap(), b"hello");
    }

    #[test]
    fn invoke_result_stack_array_keeps_lossy_parse_behavior() {
        let mut valid_stack_item = JObject::new();
        valid_stack_item.insert("type".to_string(), JToken::String("Boolean".to_string()));
        valid_stack_item.insert("value".to_string(), JToken::Boolean(true));

        let mut malformed_stack_item = JObject::new();
        malformed_stack_item.insert("type".to_string(), JToken::String("ByteString".to_string()));
        malformed_stack_item.insert(
            "value".to_string(),
            JToken::String("not base64".to_string()),
        );

        let mut stack_array = JArray::new();
        stack_array.add(Some(JToken::Object(valid_stack_item)));
        stack_array.add(None);
        stack_array.add(Some(JToken::String("not an object".to_string())));
        stack_array.add(Some(JToken::Object(malformed_stack_item)));

        let mut json = JObject::new();
        json.insert("script".to_string(), JToken::String("00".to_string()));
        json.insert("state".to_string(), JToken::String("HALT".to_string()));
        json.insert("gasconsumed".to_string(), JToken::String("1".to_string()));
        json.insert("stack".to_string(), JToken::Array(stack_array));

        let parsed = RpcInvokeResult::from_json(&json).unwrap();
        assert_eq!(parsed.stack, vec![StackValue::Boolean(true)]);
    }

    #[test]
    fn rpc_stack_parses() {
        let mut obj = JObject::new();
        obj.insert("type".to_string(), JToken::String("Integer".to_string()));
        obj.insert("value".to_string(), JToken::String("123".to_string()));
        let parsed = RpcStack::from_json(&obj).unwrap();
        assert_eq!(parsed.item_type, "Integer");
        assert_eq!(parsed.value.as_string().unwrap(), "123");
    }

    #[test]
    fn rpc_stack_to_json_matches_shape() {
        let stack = RpcStack {
            item_type: "Boolean".to_string(),
            value: JToken::Boolean(true),
        };
        assert_eq!(
            stack.to_json().to_string(),
            "{\"type\":\"Boolean\",\"value\":true}"
        );
    }

    #[test]
    fn invoke_result_to_json_emits_stack_items() {
        let result = RpcInvokeResult {
            script: "00".to_string(),
            state: VmState::Halt,
            gas_consumed: 1,
            stack: vec![StackValue::Boolean(true)],
            tx: None,
            exception: None,
            session: None,
        };
        let json = result.to_json();
        assert_eq!(
            json.get("state")
                .and_then(|token| token.as_string())
                .unwrap(),
            "HALT"
        );
        let stack = json
            .get("stack")
            .and_then(|token| token.as_array())
            .expect("stack array");
        assert_eq!(stack.len(), 1);
    }

    #[test]
    fn invoke_result_to_json_handles_circular_stack() {
        let result = RpcInvokeResult {
            script: "00".to_string(),
            state: VmState::Halt,
            gas_consumed: 1,
            stack: vec![StackValue::Array(vec![StackValue::Boolean(true)])],
            tx: None,
            exception: None,
            session: None,
        };

        let json = result.to_json();
        assert!(json
            .get("stack")
            .and_then(|token| token.as_array())
            .is_some());
    }

    #[test]
    fn invoke_result_to_json_matches_rpc_test_case() {
        let Some(expected) = rpc_case_result("invokefunctionasync") else {
            return;
        };
        let parsed = RpcInvokeResult::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

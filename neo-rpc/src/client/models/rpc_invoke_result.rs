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

use super::vm_state_utils::{vm_state_from_str, vm_state_to_string};
use neo_json::{JArray, JObject, JToken};
use neo_vm::{StackItem, VMState};

/// RPC invoke result matching C# RpcInvokeResult
#[derive(Debug, Clone)]
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
    pub tx: Option<String>,

    /// Exception message if any
    pub exception: Option<String>,

    /// Session ID if available
    pub session: Option<String>,
}

impl RpcInvokeResult {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let script = json
            .get("script")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'script' field")?
            .to_string();

        let state_str = json
            .get("state")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'state' field")?;
        let state = vm_state_from_str(&state_str)
            .ok_or_else(|| format!("Invalid VM state: {}", state_str))?;

        let gas_consumed_str = json
            .get("gasconsumed")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'gasconsumed' field")?;
        let gas_consumed = gas_consumed_str
            .parse::<i64>()
            .map_err(|_| format!("Invalid gas consumed value: {}", gas_consumed_str))?;

        let exception = json
            .get("exception")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let session = json
            .get("session")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        let tx = json
            .get("tx")
            .and_then(|v| v.as_string())
            .map(|s| s.to_string());

        // Try to parse stack items
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
    /// Matches C# ToJson
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

        let stack_json = self
            .stack
            .iter()
            .map(super::super::utility::stack_item_to_json)
            .collect::<Result<Vec<_>, _>>()
            .map(|items| {
                let values: Vec<JToken> = items.into_iter().map(JToken::Object).collect();
                JToken::Array(JArray::from(values))
            })
            .unwrap_or_else(|_| {
                JToken::String("error: recursive reference".to_string())
            });
        json.insert("stack".to_string(), stack_json);

        if let Some(tx) = &self.tx {
            if !tx.is_empty() {
                json.insert("tx".to_string(), JToken::String(tx.clone()));
            }
        }

        json
    }
}

/// RPC stack item representation matching C# RpcStack
#[derive(Debug, Clone)]
pub struct RpcStack {
    /// Stack item type
    pub item_type: String,

    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let item_type = json
            .get("type")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'type' field")?
            .to_string();

        let value = json.get("value").ok_or("Missing 'value' field")?.clone();

        Ok(Self { item_type, value })
    }

    /// Converts to JSON
    /// Matches C# ToJson
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
    use neo_json::{JArray, JToken};
    use neo_vm::stack_item::Array;
    use std::fs;
    use std::path::PathBuf;

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
        assert_eq!(parsed.state, VMState::HALT);
        assert_eq!(parsed.gas_consumed, 1);
        assert_eq!(parsed.stack.len(), 1);
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
        assert_eq!(parsed.stack[0].as_bytes().unwrap(), b"hello");
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
            state: VMState::HALT,
            gas_consumed: 1,
            stack: vec![StackItem::from_bool(true)],
            tx: None,
            exception: None,
            session: None,
        };
        let json = result.to_json();
        assert_eq!(
            json.get("state").and_then(|token| token.as_string()).unwrap(),
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
        let array = Array::new_untracked(Vec::new());
        let item = StackItem::Array(array.clone());
        array.push(item.clone()).expect("push");

        let result = RpcInvokeResult {
            script: "00".to_string(),
            state: VMState::HALT,
            gas_consumed: 1,
            stack: vec![item],
            tx: None,
            exception: None,
            session: None,
        };

        let json = result.to_json();
        assert_eq!(
            json.get("stack").and_then(|token| token.as_string()).unwrap(),
            "error: recursive reference"
        );
    }

    fn load_rpc_case_result(name: &str) -> JObject {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
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
    fn invoke_result_to_json_matches_rpc_test_case() {
        let expected = load_rpc_case_result("invokefunctionasync");
        let parsed = RpcInvokeResult::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}

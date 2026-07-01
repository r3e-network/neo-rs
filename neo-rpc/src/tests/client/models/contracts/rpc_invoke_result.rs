use super::super::rpc_stack::RpcStack;
use super::super::test_fixtures::rpc_case_result;
use super::*;
use neo_serialization::json::{JArray, JToken};
use neo_vm_rs::{StackValue, VmState, stack_value_as_bytes};

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
    assert!(
        json.get("stack")
            .and_then(|token| token.as_array())
            .is_some()
    );
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

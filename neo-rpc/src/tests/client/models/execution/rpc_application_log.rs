use super::*;
use crate::types::test_fixtures::rpc_case_result;
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

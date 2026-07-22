use super::*;
use crate::types::test_fixtures::rpc_case_result_array;
use neo_serialization::json::{JArray, JToken};

#[test]
fn rpc_plugin_roundtrip() {
    let plugin = RpcPlugin {
        name: "RpcServer".into(),
        version: "1.0.0".into(),
        interfaces: vec!["ISmartContract".into(), "IBlock".into()],
        category: Some("Rpc".into()),
    };

    let json = plugin.to_json();
    let parsed = RpcPlugin::from_json(&json).expect("plugin");
    assert_eq!(parsed.name, plugin.name);
    assert_eq!(parsed.version, plugin.version);
    assert_eq!(parsed.interfaces, plugin.interfaces);
    assert_eq!(parsed.category, plugin.category);
}

#[test]
fn rpc_plugin_defaults_to_empty_interfaces() {
    let mut json = JObject::new();
    json.insert("name".to_string(), JToken::String("Empty".into()));
    json.insert("version".to_string(), JToken::String("0.0.1".into()));

    let parsed = RpcPlugin::from_json(&json).expect("plugin");
    assert!(parsed.interfaces.is_empty());
    assert!(parsed.category.is_none());

    json.insert("interfaces".to_string(), JToken::Boolean(true));
    let parsed = RpcPlugin::from_json(&json).expect("plugin");
    assert!(parsed.interfaces.is_empty());
}

#[test]
fn rpc_plugin_rejects_empty_or_non_string_interface_entries() {
    let mut json = JObject::new();
    json.insert("name".to_string(), JToken::String("Bad".into()));
    json.insert("version".to_string(), JToken::String("0.0.1".into()));

    let mut empty_slot = JArray::new();
    empty_slot.add(None);
    json.insert("interfaces".to_string(), JToken::Array(empty_slot));
    let err = RpcPlugin::from_json(&json).expect_err("empty slot should fail");
    assert_eq!(err.to_string(), "Interface entry must be a string");

    json.insert(
        "interfaces".to_string(),
        JToken::Array(JArray::from(vec![JToken::Number(1.0)])),
    );
    let err = RpcPlugin::from_json(&json).expect_err("non-string should fail");
    assert_eq!(err.to_string(), "Interface entry must be a string");
}

#[test]
fn plugins_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result_array("listpluginsasync") else {
        return;
    };
    let parsed = expected
        .children()
        .iter()
        .filter_map(|entry| entry.as_ref())
        .filter_map(|token| token.as_object())
        .filter_map(|obj| RpcPlugin::from_json(obj).ok())
        .collect::<Vec<_>>();
    let actual = JArray::from(
        parsed
            .iter()
            .map(|plugin| JToken::Object(plugin.to_json()))
            .collect::<Vec<_>>(),
    );
    assert_eq!(expected.to_string(), actual.to_string());
}

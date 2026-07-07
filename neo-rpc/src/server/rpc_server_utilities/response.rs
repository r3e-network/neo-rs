//! Response construction helpers for utility RPC methods.

use serde_json::{Value, json};

pub(super) fn plugin_entry_to_json(name: &str, version: &str, interfaces: &[&str]) -> Value {
    json!({
        "name": name,
        "version": version,
        "interfaces": interface_values(interfaces),
    })
}

pub(super) fn plugins_to_json(mut plugins: Vec<Value>) -> Value {
    plugins.sort_by(|a, b| {
        let a_name = a.get("name").and_then(Value::as_str).unwrap_or("");
        let b_name = b.get("name").and_then(Value::as_str).unwrap_or("");
        a_name.cmp(b_name)
    });
    Value::Array(plugins)
}

pub(super) fn service_entry_to_json(
    name: &str,
    interfaces: &[&str],
    methods: Vec<String>,
    enabled: bool,
    ready: bool,
    status: Value,
) -> Value {
    json!({
        "name": name,
        "enabled": enabled,
        "ready": ready,
        "interfaces": interface_values(interfaces),
        "methods": methods,
        "status": status,
    })
}

pub(super) fn services_to_json(services: Vec<Value>) -> Value {
    Value::Array(services)
}

pub(super) fn validate_address_to_json(address: &str, is_valid: bool) -> Value {
    json!({
        "address": address,
        "isvalid": is_valid})
}

fn interface_values(interfaces: &[&str]) -> Vec<Value> {
    interfaces
        .iter()
        .map(|value| Value::String((*value).to_string()))
        .collect()
}

use serde_json::Value;

mod plugins;
mod services;

fn interface_values(interfaces: &[&str]) -> Vec<Value> {
    interfaces
        .iter()
        .map(|value| Value::String((*value).to_string()))
        .collect()
}

use super::super::utility::required_string;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use neo_vm_rs::VmState;

pub fn vm_state_to_string(state: VmState) -> String {
    match state {
        VmState::None => "NONE",
        VmState::Halt => "HALT",
        VmState::Fault => "FAULT",
        VmState::Break => "BREAK",
    }
    .to_string()
}

pub fn vm_state_from_str(value: &str) -> Option<VmState> {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "NONE" => Some(VmState::None),
        "HALT" => Some(VmState::Halt),
        "FAULT" => Some(VmState::Fault),
        "BREAK" => Some(VmState::Break),
        _ => None,
    }
}

pub(super) fn parse_vm_state_field(json: &JObject, field: &str) -> CoreResult<VmState> {
    let value = required_string(json, field).map_err(|e| CoreError::other(e.to_string()))?;
    vm_state_from_str(&value).ok_or_else(|| CoreError::other(format!("Invalid VM state: {value}")))
}

pub(super) fn insert_vm_state_field(json: &mut JObject, field: &str, state: VmState) {
    json.insert(field.to_string(), JToken::String(vm_state_to_string(state)));
}

pub(super) fn parse_gas_consumed_field(json: &JObject) -> CoreResult<i64> {
    let value =
        required_string(json, "gasconsumed").map_err(|e| CoreError::other(e.to_string()))?;
    value
        .parse::<i64>()
        .map_err(|_| CoreError::other(format!("Invalid gas consumed value: {value}")))
}

pub(super) fn insert_gas_consumed_field(json: &mut JObject, gas_consumed: i64) {
    json.insert(
        "gasconsumed".to_string(),
        JToken::String(gas_consumed.to_string()),
    );
}

#[cfg(test)]
#[path = "../../../tests/client/models/support/vm_state_utils.rs"]
mod tests;

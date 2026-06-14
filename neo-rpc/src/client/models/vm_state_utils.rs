use super::super::utility::required_string;
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use neo_vm_rs::VmState;

pub fn vm_state_to_string(state: VmState) -> String {
    state
        .final_name()
        .expect("RPC VM state must be final")
        .to_string()
}

pub fn vm_state_from_str(value: &str) -> Option<VmState> {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "HALT" => Some(VmState::Halt),
        "FAULT" => Some(VmState::Fault),
        _ => None,
    }
}

pub(super) fn parse_vm_state_field(json: &JObject, field: &str) -> CoreResult<VmState> {
    let value = required_string(json, field).map_err(|e| CoreError::other(e.to_string()))?;
    vm_state_from_str(&value)
        .ok_or_else(|| CoreError::other(format!("Invalid VM state: {value}")))
}

pub(super) fn insert_vm_state_field(json: &mut JObject, field: &str, state: VmState) {
    json.insert(field.to_string(), JToken::String(vm_state_to_string(state)));
}

pub(super) fn parse_gas_consumed_field(json: &JObject) -> CoreResult<i64> {
    let value = required_string(json, "gasconsumed").map_err(|e| CoreError::other(e.to_string()))?;
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
mod tests {
    use super::*;

    #[test]
    fn parses_final_vm_state_case_insensitive() {
        assert_eq!(vm_state_from_str("halt"), Some(VmState::Halt));
        assert_eq!(vm_state_from_str("FAULT"), Some(VmState::Fault));
        assert!(vm_state_from_str("running").is_none());
        assert!(vm_state_from_str("paused").is_none());
        assert!(vm_state_from_str("unknown").is_none());
    }

    #[test]
    fn vm_state_to_string_roundtrip() {
        for state in [VmState::Halt, VmState::Fault] {
            let text = vm_state_to_string(state);
            assert_eq!(vm_state_from_str(&text), Some(state));
        }
    }

    #[test]
    fn vm_state_field_helpers_preserve_rpc_errors_and_output() {
        let mut json = JObject::new();
        json.insert("state".to_string(), JToken::String("halt".to_string()));

        assert_eq!(parse_vm_state_field(&json, "state"), Ok(VmState::Halt));

        let mut output = JObject::new();
        insert_vm_state_field(&mut output, "vmstate", VmState::Fault);
        assert_eq!(
            output.get("vmstate").and_then(JToken::as_string).as_deref(),
            Some("FAULT")
        );

        json.insert("state".to_string(), JToken::String("running".to_string()));
        assert_eq!(
            parse_vm_state_field(&json, "state")
                .expect_err("invalid VM state")
                .to_string(),
            "Invalid VM state: running"
        );

        let missing = JObject::new();
        assert_eq!(
            parse_vm_state_field(&missing, "state")
                .expect_err("missing VM state")
                .to_string(),
            "Missing or invalid 'state' field"
        );
    }

    #[test]
    fn gas_consumed_field_helpers_preserve_rpc_errors_and_output() {
        let mut json = JObject::new();
        json.insert("gasconsumed".to_string(), JToken::String("-7".to_string()));

        assert_eq!(parse_gas_consumed_field(&json), Ok(-7));

        let mut output = JObject::new();
        insert_gas_consumed_field(&mut output, 42);
        assert_eq!(
            output
                .get("gasconsumed")
                .and_then(JToken::as_string)
                .as_deref(),
            Some("42")
        );

        json.insert("gasconsumed".to_string(), JToken::String("bad".to_string()));
        assert_eq!(
            parse_gas_consumed_field(&json)
                .expect_err("invalid gas consumed")
                .to_string(),
            "Invalid gas consumed value: bad"
        );

        let missing = JObject::new();
        assert_eq!(
            parse_gas_consumed_field(&missing)
                .expect_err("missing gas consumed")
                .to_string(),
            "Missing or invalid 'gasconsumed' field"
        );
    }
}

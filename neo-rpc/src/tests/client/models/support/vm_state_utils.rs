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

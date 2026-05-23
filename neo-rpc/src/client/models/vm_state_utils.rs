use neo_vm_rs::VmState;

pub fn vm_state_to_string(state: VmState) -> String {
    match state {
        VmState::Halt => "HALT",
        VmState::Fault => "FAULT",
    }
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
}

use neo_vm::VMState;

pub fn vm_state_to_string(state: VMState) -> String {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
    .to_string()
}

pub fn vm_state_from_str(value: &str) -> Option<VMState> {
    let normalized = value.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "NONE" => Some(VMState::NONE),
        "HALT" => Some(VMState::HALT),
        "FAULT" => Some(VMState::FAULT),
        "BREAK" => Some(VMState::BREAK),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_vm_state_case_insensitive() {
        assert_eq!(vm_state_from_str("halt"), Some(VMState::HALT));
        assert_eq!(vm_state_from_str("FAULT"), Some(VMState::FAULT));
        assert_eq!(vm_state_from_str(" Break "), Some(VMState::BREAK));
        assert_eq!(vm_state_from_str("none"), Some(VMState::NONE));
        assert!(vm_state_from_str("unknown").is_none());
    }

    #[test]
    fn vm_state_to_string_roundtrip() {
        for state in [VMState::HALT, VMState::FAULT, VMState::BREAK, VMState::NONE] {
            let text = vm_state_to_string(state);
            assert_eq!(vm_state_from_str(&text), Some(state));
        }
    }
}

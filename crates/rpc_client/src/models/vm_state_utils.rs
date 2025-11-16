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

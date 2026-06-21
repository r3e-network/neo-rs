use super::*;

#[test]
fn test_jump_table_creation() {
    let jump_table = JumpTable::new();

    // Check that all opcodes have handlers
    for opcode in OpCode::ALL {
        assert!(
            jump_table.get(opcode).is_some(),
            "No handler for opcode: {:?}",
            opcode
        );
    }
}

#[test]
fn test_jump_table_register() -> Result<(), Box<dyn std::error::Error>> {
    let mut jump_table = JumpTable::new();

    // Define a custom handler
    fn custom_handler(
        _engine: &mut ExecutionEngine,
        _instruction: &Instruction,
    ) -> VmResult<()> {
        Ok(())
    }

    // Register the custom handler
    jump_table.register(OpCode::NOP, custom_handler);

    // Check that the handler was registered
    assert_eq!(
        jump_table.get(OpCode::NOP).ok_or("Index out of bounds")? as *const () as usize,
        custom_handler as *const () as usize
    );
    Ok(())
}

#[test]
fn test_jump_table_index() -> Result<(), Box<dyn std::error::Error>> {
    let mut jump_table = JumpTable::new();

    // Define a custom handler
    fn custom_handler(
        _engine: &mut ExecutionEngine,
        _instruction: &Instruction,
    ) -> VmResult<()> {
        Ok(())
    }

    // Set the handler using the index operator
    jump_table[OpCode::NOP] = custom_handler;

    // Check that the handler was set
    assert_eq!(
        jump_table.get(OpCode::NOP).ok_or("Index out of bounds")? as *const () as usize,
        custom_handler as *const () as usize
    );
    Ok(())
}

#[test]
fn test_jump_table_default() {
    // Get the default jump table
    let jump_table = JumpTable::default();

    // Check that all opcodes have handlers
    for opcode in OpCode::ALL {
        assert!(
            jump_table.get(opcode).is_some(),
            "No handler for opcode: {:?}",
            opcode
        );
    }
}

/// The pre-HF_Gorgon table overrides SHL/SHR + HASKEY/PICKITEM/SETITEM/REMOVE
/// with the pre-fork handlers, and leaves every other opcode as the default.
#[test]
fn not_gorgon_table_overrides_pre_fork_opcodes() {
    let default = JumpTable::default();
    let not_gorgon = JumpTable::not_gorgon();
    let overridden = [
        OpCode::SHL,
        OpCode::SHR,
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
    ];
    for opcode in OpCode::ALL {
        assert!(
            not_gorgon.get(opcode).is_some(),
            "missing handler: {opcode:?}"
        );
        let same = not_gorgon.get(opcode).map(|h| h as usize)
            == default.get(opcode).map(|h| h as usize);
        if overridden.contains(&opcode) {
            assert!(!same, "{opcode:?} should be overridden in not_gorgon");
        } else {
            assert!(same, "{opcode:?} should match the default table");
        }
    }
    // C# v3.10.0 `ComposeNotEchidnaJumpTable` only changes SUBSTR. Rust does
    // not reproduce the memory-unsafe SUBSTR distinction, so NotEchidna must
    // not inherit the unrelated pre-Gorgon overrides.
    let not_echidna = JumpTable::not_echidna();
    for opcode in OpCode::ALL {
        assert_eq!(
            not_echidna.get(opcode).map(|h| h as usize),
            default.get(opcode).map(|h| h as usize),
            "not_echidna must equal the default table for {opcode:?}"
        );
    }
}

#[test]
fn test_jump_table_invalid_opcode() {
    let jump_table = JumpTable::new();

    // Create a mock engine and instruction
    let mut engine = ExecutionEngine::new(None);
    let instruction = Instruction::new(OpCode::NOP, &[]);

    let mut jump_table = jump_table.clone();
    jump_table.handlers[OpCode::NOP as usize] = None;

    // Execute the instruction
    let result = jump_table.execute(&mut engine, &instruction);

    assert!(result.is_err());
}

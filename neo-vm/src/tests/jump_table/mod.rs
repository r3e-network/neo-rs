//! # neo-vm::tests::jump_table
//!
//! Test module grouping Opcode dispatch tables and instruction implementations.
//! coverage for neo-vm.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-vm; it may assemble fixtures but
//! must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

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
    fn custom_handler(_engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
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
    fn custom_handler(_engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
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

/// The pre-543 table overrides HASKEY/PICKITEM/SETITEM/REMOVE with the pre-fork
/// handlers, and leaves every other opcode as the default. SHL/SHR are NOT
/// overridden: the C# VM has a single SHL/SHR behavior (no `HF_Gorgon` split).
#[test]
fn not_gorgon_table_overrides_pre_fork_opcodes() {
    let default = JumpTable::default();
    let not_gorgon = JumpTable::not_gorgon();
    let overridden = [
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
        let same =
            not_gorgon.get(opcode).map(|h| h as usize) == default.get(opcode).map(|h| h as usize);
        if overridden.contains(&opcode) {
            assert!(!same, "{opcode:?} should be overridden in not_gorgon");
        } else {
            assert!(same, "{opcode:?} should match the default table");
        }
    }
    // C# v3.10.1 `ComposeNotEchidnaJumpTable` only changes SUBSTR. Rust does
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

fn engine_with_items(items: Vec<StackItem>) -> ExecutionEngine {
    use crate::script::Script;
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
        .expect("load test script");
    let ctx = engine.current_context_mut().expect("current context");
    for item in items {
        ctx.push(item).expect("push test item");
    }
    engine
}

#[test]
fn get_integer_faults_on_buffer_and_null_like_csharp() {
    // C# StackItem.GetInteger(): a Buffer is not a PrimitiveType (no GetInteger
    // override) and faults; Null faults; the Integer/Boolean/ByteString
    // primitives convert. Rust's into_int() instead coerces a <=32-byte Buffer.
    assert!(get_integer(StackItem::from_buffer(vec![0x05])).is_err());
    assert!(get_integer(StackItem::Null).is_err());
    assert_eq!(
        get_integer(StackItem::from_i64(7)).unwrap(),
        BigInt::from(7)
    );
    assert_eq!(
        get_integer(StackItem::from_bool(true)).unwrap(),
        BigInt::from(1)
    );
}

#[test]
fn count_opcodes_fault_on_buffer_operand_like_csharp() {
    // NEWBUFFER (splice), PICK (stack) and NEWARRAY (compound) each read their
    // size/index/count operand via GetInteger, which faults on a Buffer in the
    // reference VM. Rust previously coerced a <=32-byte Buffer to an integer and
    // proceeded, diverging from C#.
    let jt = JumpTable::default();
    let buf = || StackItem::from_buffer(vec![0x01]);

    let mut e = engine_with_items(vec![buf()]);
    assert!(
        jt.execute(&mut e, &Instruction::new(OpCode::NEWBUFFER, &[]))
            .is_err(),
        "NEWBUFFER with a Buffer size operand must fault"
    );

    let mut e = engine_with_items(vec![StackItem::from_i64(1), StackItem::from_i64(2), buf()]);
    assert!(
        jt.execute(&mut e, &Instruction::new(OpCode::PICK, &[]))
            .is_err(),
        "PICK with a Buffer index operand must fault"
    );

    let mut e = engine_with_items(vec![buf()]);
    assert!(
        jt.execute(&mut e, &Instruction::new(OpCode::NEWARRAY, &[]))
            .is_err(),
        "NEWARRAY with a Buffer count operand must fault"
    );
}

#[test]
fn collection_key_opcodes_fault_on_buffer_key_like_csharp() {
    // C# pops the collection key via Pop<PrimitiveType>(); a Buffer is not a
    // PrimitiveType, so PICKITEM/SETITEM/HASKEY/REMOVE and the PACKMAP entries
    // fault on a Buffer key. The gate runs right after the key is popped (before
    // the collection), so the dummy beneath the key is never examined.
    let jt = JumpTable::default();
    let buf_key = || StackItem::from_buffer(vec![0x01]);
    let dummy = || StackItem::from_i64(0);

    // PICKITEM/HASKEY/REMOVE: stack = [collection, key]; Buffer key on top.
    for op in [OpCode::PICKITEM, OpCode::HASKEY, OpCode::REMOVE] {
        let mut e = engine_with_items(vec![dummy(), buf_key()]);
        assert!(
            jt.execute(&mut e, &Instruction::new(op, &[])).is_err(),
            "{op:?} with a Buffer key must fault"
        );
    }

    // SETITEM: stack = [collection, key, value]; value on top, Buffer key below.
    let mut e = engine_with_items(vec![dummy(), buf_key(), dummy()]);
    assert!(
        jt.execute(&mut e, &Instruction::new(OpCode::SETITEM, &[]))
            .is_err(),
        "SETITEM with a Buffer key must fault"
    );

    // PACKMAP size=1: stack = [value, key, size]; pops size, then the Buffer key.
    let mut e = engine_with_items(vec![dummy(), buf_key(), StackItem::from_i64(1)]);
    assert!(
        jt.execute(&mut e, &Instruction::new(OpCode::PACKMAP, &[]))
            .is_err(),
        "PACKMAP with a Buffer key must fault"
    );
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

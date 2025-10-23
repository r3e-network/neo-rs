//! C# parity tests for the Neo VM reference counter.

use neo_vm::{
    debugger::Debugger, execution_engine::ExecutionEngine, execution_engine_limits::ExecutionEngineLimits,
    op_code::OpCode, reference_counter::ReferenceCounter, script_builder::ScriptBuilder,
    stack_item::array::Array, stack_item::stack_item_type::StackItemType, stack_item::StackItem,
    vm_state::VMState,
};

fn new_debugger_with_script(builder: &ScriptBuilder) -> Debugger {
    let script = builder.to_script();
    let engine = ExecutionEngine::new(None);
    let mut debugger = Debugger::new(engine);
    debugger
        .engine_mut()
        .load_script(script, -1, 0)
        .expect("script should load");
    debugger
}

#[test]
fn test_circular_references() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_instruction(OpCode::INITSSLOT, &[1])
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::DUP)
        .emit_opcode(OpCode::DUP)
        .emit_opcode(OpCode::APPEND)
        .emit_opcode(OpCode::DUP)
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::STSFLD0)
        .emit_opcode(OpCode::LDSFLD0)
        .emit_opcode(OpCode::APPEND)
        .emit_opcode(OpCode::LDSFLD0)
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::TUCK)
        .emit_opcode(OpCode::APPEND)
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::TUCK)
        .emit_opcode(OpCode::APPEND)
        .emit_opcode(OpCode::LDSFLD0)
        .emit_opcode(OpCode::APPEND)
        .emit_opcode(OpCode::PUSHNULL)
        .emit_opcode(OpCode::STSFLD0)
        .emit_opcode(OpCode::DUP)
        .emit_push_int(1)
        .emit_opcode(OpCode::REMOVE)
        .emit_opcode(OpCode::STSFLD0)
        .emit_opcode(OpCode::RET);

    let mut debugger = new_debugger_with_script(&builder);

    let expected_counts: [usize; 29] = [
        1, 2, 2, 3, 4, 3, 4, 5, 5, 4, 5, 4, 5, 6, 6, 7, 6, 7, 7, 8, 7, 8, 7, 8, 7, 8, 9, 6, 5,
    ];

    for expected in expected_counts {
        assert_eq!(debugger.step_into(), VMState::BREAK);
        assert_eq!(debugger.engine().reference_counter().count(), expected);
    }

    assert_eq!(debugger.execute(), VMState::HALT);
    assert_eq!(debugger.engine().reference_counter().count(), 4);
}

#[test]
fn test_remove_referrer() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_instruction(OpCode::INITSSLOT, &[1])
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::DUP)
        .emit_push_int(0)
        .emit_opcode(OpCode::NEWARRAY)
        .emit_opcode(OpCode::STSFLD0)
        .emit_opcode(OpCode::LDSFLD0)
        .emit_opcode(OpCode::APPEND)
        .emit_opcode(OpCode::DROP)
        .emit_opcode(OpCode::RET);

    let mut debugger = new_debugger_with_script(&builder);

    let expected_counts: [usize; 10] = [1, 2, 2, 3, 4, 4, 3, 4, 3, 2];

    for expected in expected_counts {
        assert_eq!(debugger.step_into(), VMState::BREAK);
        assert_eq!(debugger.engine().reference_counter().count(), expected);
    }

    assert_eq!(debugger.execute(), VMState::HALT);
    assert_eq!(debugger.engine().reference_counter().count(), 1);
}

#[test]
fn test_check_zero_referred_with_array() {
    let mut builder = ScriptBuilder::new();
    let limits = ExecutionEngineLimits::default();
    let max_stack = limits.max_stack_size as i64;

    builder
        .emit_push_int(max_stack - 1)
        .emit_opcode(OpCode::NEWARRAY);

    {
        let script = builder.to_script();
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(script, -1, 0)
            .expect("script should load");
        assert_eq!(engine.reference_counter().count(), 0);
        assert_eq!(engine.execute(), VMState::HALT);
        assert_eq!(engine.reference_counter().count(), limits.max_stack_size as usize);
    }

    builder.emit_opcode(OpCode::PUSH1);

    {
        let script = builder.to_script();
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(script, -1, 0)
            .expect("script should load");
        assert_eq!(engine.reference_counter().count(), 0);
        assert_eq!(engine.execute(), VMState::FAULT);
        assert_eq!(engine.reference_counter().count(), (limits.max_stack_size + 1) as usize);
    }
}

#[test]
fn test_check_zero_referred() {
    let mut builder = ScriptBuilder::new();

    let limits = ExecutionEngineLimits::default();
    for _ in 0..limits.max_stack_size {
        builder.emit_opcode(OpCode::PUSH1);
    }

    {
        let script = builder.to_script();
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(script, -1, 0)
            .expect("script should load");
        assert_eq!(engine.reference_counter().count(), 0);
        assert_eq!(engine.execute(), VMState::HALT);
        assert_eq!(engine.reference_counter().count(), limits.max_stack_size as usize);
    }

    builder.emit_opcode(OpCode::PUSH1);

    {
        let script = builder.to_script();
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(script, -1, 0)
            .expect("script should load");
        assert_eq!(engine.reference_counter().count(), 0);
        assert_eq!(engine.execute(), VMState::FAULT);
        assert_eq!(engine.reference_counter().count(), (limits.max_stack_size + 1) as usize);
    }
}

#[test]
fn test_array_no_push() {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(OpCode::RET);

    let script = builder.to_script();
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(script, -1, 0)
        .expect("script should load");
    assert_eq!(engine.reference_counter().count(), 0);

    let mut array_items = Vec::new();
    for value in 1..=4 {
        array_items.push(StackItem::from_int(value));
    }

    let array =
        Array::new(array_items, Some(engine.reference_counter().clone()));
    assert_eq!(array.stack_item_type(), StackItemType::Array);
    assert_eq!(array.len(), engine.reference_counter().count());

    assert_eq!(engine.execute(), VMState::HALT);
    assert_eq!(array.len(), engine.reference_counter().count());
}

#[test]
fn test_invalid_reference_stack_item() {
    let counter = ReferenceCounter::new();
    let mut array = Array::new(Vec::new(), Some(counter.clone()));
    let mut array_without_counter = Array::new(Vec::new(), None);

    for value in 0..10 {
        array_without_counter
            .push(StackItem::from_int(value))
            .expect("push should succeed");
    }

    let err = array
        .push(StackItem::Array(array_without_counter))
        .expect_err("pushing array without counter should fail");

    assert!(
        matches!(err, neo_vm::VmError::InvalidOperation { .. }),
        "expected InvalidOperation error, got {err:?}"
    );
}

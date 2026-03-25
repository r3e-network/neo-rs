use neo_vm::execution_engine::{ExecutionEngine, VMState};
use neo_vm::op_code::OpCode;
use neo_vm::script::Script;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::StackItem;

#[test]
fn test_division_by_zero() {
    let mut engine = ExecutionEngine::new(None);
    let script = ScriptBuilder::new()
        .emit_opcode(OpCode::PUSH1)
        .emit_opcode(OpCode::PUSH0)
        .emit_opcode(OpCode::DIV)
        .to_script();

    engine.load_script(script, -1, 0).unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::FAULT);
}

#[test]
fn test_modulo_by_zero() {
    let mut engine = ExecutionEngine::new(None);
    let script = ScriptBuilder::new()
        .emit_opcode(OpCode::PUSH1)
        .emit_opcode(OpCode::PUSH0)
        .emit_opcode(OpCode::MOD)
        .to_script();

    engine.load_script(script, -1, 0).unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::FAULT);
}

#[test]
fn test_newarray_t_allows_any_type_and_uses_null_defaults() {
    let mut engine = ExecutionEngine::new(None);
    let script = Script::new_relaxed(vec![
        OpCode::PUSH2 as u8,
        OpCode::NEWARRAY_T as u8,
        0x00,
        OpCode::RET as u8,
    ]);

    engine.load_script(script, -1, 0).unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    let top = engine.result_stack().peek(0).unwrap().clone();
    let StackItem::Array(array) = top else {
        panic!("expected array result, got {top:?}");
    };
    let items = array.items();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(StackItem::is_null));
}

#[test]
fn test_setitem_accepts_struct_values_with_engine_reference_counter() {
    let mut engine = ExecutionEngine::new(None);
    let script = Script::new_relaxed(vec![
        OpCode::PUSH1 as u8,
        OpCode::NEWARRAY as u8,
        OpCode::DUP as u8,
        OpCode::PUSH0 as u8,
        OpCode::NEWSTRUCT0 as u8,
        OpCode::SETITEM as u8,
        OpCode::RET as u8,
    ]);

    engine.load_script(script, -1, 0).unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    let top = engine.result_stack().peek(0).unwrap().clone();
    let StackItem::Array(array) = top else {
        panic!("expected array result, got {top:?}");
    };
    let items = array.items();
    assert_eq!(items.len(), 1);
    assert!(matches!(items.first(), Some(StackItem::Struct(_))));
}

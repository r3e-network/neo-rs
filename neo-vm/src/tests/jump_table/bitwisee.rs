use super::*;
use crate::script::Script;
use num_bigint::BigInt;

fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
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

fn instruction(opcode: OpCode) -> Instruction {
    Instruction::new(opcode, &[])
}

fn pop(engine: &mut ExecutionEngine) -> StackItem {
    engine
        .current_context_mut()
        .expect("current context")
        .pop()
        .expect("result item")
}

#[test]
fn and_accepts_buffer_via_byte_string_semantics() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_buffer(vec![0xff]),
        StackItem::from_byte_string(vec![0x00, 0x80]),
    ]);

    and(&mut engine, &instruction(OpCode::AND)).expect("AND succeeds");

    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(-32768));
}

#[test]
fn xor_uses_signed_extension_for_mixed_width_operands() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_byte_string(vec![0xff]),
        StackItem::from_byte_string(vec![0x00, 0x80]),
    ]);

    xor(&mut engine, &instruction(OpCode::XOR)).expect("XOR succeeds");

    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(32767));
}

#[test]
fn equal_does_not_coerce_buffer_to_byte_string() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_buffer(vec![0x01]),
        StackItem::from_byte_string(vec![0x01]),
    ]);

    equal(&mut engine, &instruction(OpCode::EQUAL)).expect("EQUAL succeeds");

    assert!(!pop(&mut engine).as_bool().unwrap());
}

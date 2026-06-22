use super::*;
use crate::script::Script;
use crate::stack_item::StackItem;

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

/// C# JMP comparison opcodes read operands via `GetInteger()`, which faults on a
/// `Buffer` (not a `PrimitiveType`, no `GetInteger` override). The handlers must
/// use `get_integer`, NOT `into_int` (which coerces a <=32-byte Buffer to an
/// integer). JumpTable.Control.cs:134-135.
#[test]
fn jmpeq_faults_on_buffer_operand_like_csharp_getinteger() {
    let mut engine =
        engine_with_stack(vec![StackItem::from_i64(1), StackItem::from_buffer(vec![0x01])]);
    assert!(
        jmpeq(&mut engine, &instruction(OpCode::JMPEQ)).is_err(),
        "JMPEQ faults on a Buffer operand (C# GetInteger faults on Buffer)"
    );
}

#[test]
fn jmplt_faults_on_buffer_operand_like_csharp_getinteger() {
    let mut engine =
        engine_with_stack(vec![StackItem::from_i64(1), StackItem::from_buffer(vec![0x01])]);
    assert!(
        jmplt(&mut engine, &instruction(OpCode::JMPLT)).is_err(),
        "JMPLT faults on a Buffer operand"
    );
}

#[test]
fn jmpeq_with_unequal_integers_does_not_fault() {
    // Unequal integers => no jump taken, operand not read, no fault.
    let mut engine = engine_with_stack(vec![StackItem::from_i64(1), StackItem::from_i64(2)]);
    jmpeq(&mut engine, &instruction(OpCode::JMPEQ))
        .expect("JMPEQ with unequal integers does not fault");
}

/// C# ASSERTMSG reads the message via `GetString()` (strict UTF-8 —
/// DecoderFallback.ExceptionFallback), which faults on invalid bytes BEFORE the
/// condition is evaluated. JumpTable.Types.cs:91-96.
#[test]
fn assertmsg_faults_on_invalid_utf8_even_when_condition_true() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_bool(true),
        StackItem::from_byte_string(vec![0xff, 0xfe]),
    ]);
    assert!(
        assertmsg(&mut engine, &instruction(OpCode::ASSERTMSG)).is_err(),
        "invalid-UTF8 message faults even with a true condition (C# strict GetString)"
    );
}

#[test]
fn assertmsg_passes_with_valid_utf8_and_true_condition() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_bool(true),
        StackItem::from_byte_string(b"ok".to_vec()),
    ]);
    assertmsg(&mut engine, &instruction(OpCode::ASSERTMSG))
        .expect("valid UTF-8 message with a true condition passes");
}

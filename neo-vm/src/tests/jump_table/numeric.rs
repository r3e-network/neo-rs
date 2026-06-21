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

fn run_bool(
    left: StackItem,
    right: StackItem,
    opcode: OpCode,
    op: fn(&mut ExecutionEngine, &Instruction) -> VmResult<()>,
) -> bool {
    let mut engine = engine_with_stack(vec![left, right]);
    op(&mut engine, &instruction(opcode)).expect("comparison succeeds");
    pop(&mut engine).as_bool().expect("boolean result")
}

/// Pre-HF_Gorgon vulnerable SHL (neo-vm#567): a zero shift returns WITHOUT
/// popping the value operand, so the value is left on the stack untouched —
/// whereas the fixed handler pops the value, validates + normalizes it (e.g.
/// a Buffer becomes its Integer interpretation) and re-pushes it. The
/// observable difference (the surviving stack item) is the live divergence.
#[test]
fn vulnerable_shl_diverges_from_fixed_on_zero_shift() {
    let buffer = || StackItem::from_buffer(vec![0x07]);

    // Vulnerable: the Buffer is left untouched on the stack.
    let mut engine = engine_with_stack(vec![buffer(), StackItem::from_i64(0)]);
    shl_vulnerable(&mut engine, &instruction(OpCode::SHL))
        .expect("vulnerable SHL must not fault on a zero shift");
    assert!(
        matches!(pop(&mut engine), StackItem::Buffer(_)),
        "the value operand is left untouched (still a Buffer)"
    );

    // Fixed: the value is popped, normalized (Buffer -> integer), re-pushed.
    let mut engine = engine_with_stack(vec![buffer(), StackItem::from_i64(0)]);
    shl(&mut engine, &instruction(OpCode::SHL)).expect("fixed SHL ok");
    let top = pop(&mut engine);
    assert!(
        !matches!(top, StackItem::Buffer(_)),
        "fixed SHL normalizes the value (no longer a Buffer)"
    );
    assert_eq!(top.as_int().unwrap(), BigInt::from(7));

    // Both agree on a zero shift over an integer (identity).
    let mut engine = engine_with_stack(vec![StackItem::from_i64(7), StackItem::from_i64(0)]);
    shl_vulnerable(&mut engine, &instruction(OpCode::SHL)).expect("vulnerable SHL ok");
    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(7));
}

#[test]
fn add_accepts_buffer_as_byte_string_operand() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_buffer(vec![0x02]),
        StackItem::from_i64(3),
    ]);

    add(&mut engine, &instruction(OpCode::ADD)).expect("ADD succeeds");

    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(5));
}

#[test]
fn ordered_comparisons_keep_core_null_policy() {
    assert!(run_bool(
        StackItem::Null,
        StackItem::from_i64(1),
        OpCode::LT,
        lt
    ));
    assert!(!run_bool(
        StackItem::from_i64(1),
        StackItem::Null,
        OpCode::LT,
        lt
    ));
    assert!(run_bool(StackItem::Null, StackItem::Null, OpCode::LE, le));
    assert!(run_bool(
        StackItem::from_i64(1),
        StackItem::Null,
        OpCode::GT,
        gt
    ));
    assert!(run_bool(StackItem::Null, StackItem::Null, OpCode::GE, ge));
}

#[test]
fn numeric_equality_preserves_null_special_cases() {
    assert!(run_bool(
        StackItem::Null,
        StackItem::Null,
        OpCode::NUMEQUAL,
        numequal
    ));
    assert!(!run_bool(
        StackItem::Null,
        StackItem::from_i64(0),
        OpCode::NUMEQUAL,
        numequal
    ));
    assert!(run_bool(
        StackItem::Null,
        StackItem::from_i64(0),
        OpCode::NUMNOTEQUAL,
        numnotequal
    ));
}

#[test]
fn modpow_supports_modular_inverse() {
    let mut engine = engine_with_stack(vec![
        StackItem::from_i64(3),
        StackItem::from_i64(-1),
        StackItem::from_i64(11),
    ]);

    modpow(&mut engine, &instruction(OpCode::MODPOW)).expect("MODPOW succeeds");

    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(4));
}

#[test]
fn shift_rejects_values_above_engine_limit() {
    let mut engine = engine_with_stack(vec![StackItem::from_i64(1), StackItem::from_i64(257)]);

    assert!(shl(&mut engine, &instruction(OpCode::SHL)).is_err());
}

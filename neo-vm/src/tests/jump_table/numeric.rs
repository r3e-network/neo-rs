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

/// C# `Not` reads its operand via `GetBoolean()` (JumpTable.Numeric.cs:271-274),
/// which never faults on type — unlike `GetInteger()`. So NOT over a Buffer or
/// Null operand must NOT fault (it must produce a boolean), the opposite of the
/// numeric arithmetic opcodes which fault on Buffer/Null.
#[test]
fn not_uses_getboolean_semantics_not_getinteger() {
    // Buffer is truthy under GetBoolean => NOT => false (no fault).
    let mut engine = engine_with_stack(vec![StackItem::from_buffer(vec![0x00])]);
    not(&mut engine, &instruction(OpCode::NOT)).expect("NOT(Buffer) must not fault");
    assert!(
        !pop(&mut engine).as_bool().unwrap(),
        "NOT(Buffer) => false (Buffer is truthy)"
    );

    // Null is falsy under GetBoolean => NOT => true (no fault).
    let mut engine = engine_with_stack(vec![StackItem::Null]);
    not(&mut engine, &instruction(OpCode::NOT)).expect("NOT(Null) must not fault");
    assert!(
        pop(&mut engine).as_bool().unwrap(),
        "NOT(Null) => true (Null is falsy)"
    );
}

#[test]
fn not_on_integer_preserves_boolean_negation() {
    let mut engine = engine_with_stack(vec![StackItem::from_i64(0)]);
    not(&mut engine, &instruction(OpCode::NOT)).expect("NOT(0)");
    assert!(pop(&mut engine).as_bool().unwrap(), "NOT(0) => true");

    let mut engine = engine_with_stack(vec![StackItem::from_i64(5)]);
    not(&mut engine, &instruction(OpCode::NOT)).expect("NOT(5)");
    assert!(!pop(&mut engine).as_bool().unwrap(), "NOT(5) => false");
}

/// Pre-HF_Gorgon vulnerable SHL (neo-vm#567): a zero shift returns WITHOUT
/// popping the value operand, so the value is left on the stack untouched —
/// whereas the fixed handler pops the value and coerces it via GetInteger().
/// For a Buffer value the fixed handler now FAULTS (C# Buffer has no GetInteger
/// override → InvalidCastException), so the two paths diverge observably.
#[test]
fn vulnerable_shl_diverges_from_fixed_on_zero_shift() {
    let buffer = || StackItem::from_buffer(vec![0x07]);

    // Vulnerable: a zero shift returns without popping the value -> Buffer left.
    let mut engine = engine_with_stack(vec![buffer(), StackItem::from_i64(0)]);
    shl_vulnerable(&mut engine, &instruction(OpCode::SHL))
        .expect("vulnerable SHL must not fault on a zero shift");
    assert!(
        matches!(pop(&mut engine), StackItem::Buffer(_)),
        "the value operand is left untouched (still a Buffer)"
    );

    // Fixed: pops the value and coerces via GetInteger(); a Buffer FAULTS.
    let mut engine = engine_with_stack(vec![buffer(), StackItem::from_i64(0)]);
    assert!(
        shl(&mut engine, &instruction(OpCode::SHL)).is_err(),
        "fixed SHL faults on a Buffer value (C# GetInteger faults on Buffer)"
    );

    // Both agree on a zero shift over an integer (identity).
    let mut engine = engine_with_stack(vec![StackItem::from_i64(7), StackItem::from_i64(0)]);
    shl_vulnerable(&mut engine, &instruction(OpCode::SHL)).expect("vulnerable SHL ok");
    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(7));
    let mut engine = engine_with_stack(vec![StackItem::from_i64(7), StackItem::from_i64(0)]);
    shl(&mut engine, &instruction(OpCode::SHL)).expect("fixed SHL ok on integer");
    assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(7));
}

#[test]
fn add_faults_on_buffer_operand_like_csharp() {
    // C# ADD calls GetInteger() on each operand; a Buffer (no GetInteger
    // override) throws InvalidCastException -> FAULT. Rust must not coerce it.
    let mut engine = engine_with_stack(vec![
        StackItem::from_buffer(vec![0x02]),
        StackItem::from_i64(3),
    ]);
    assert!(
        add(&mut engine, &instruction(OpCode::ADD)).is_err(),
        "ADD with a Buffer operand must fault (C# GetInteger faults on Buffer)"
    );
}

#[test]
fn ordered_comparisons_push_false_for_any_null_like_csharp() {
    // C# Lt/Le/Gt/Ge: `if (x1.IsNull || x2.IsNull) Push(false)` — ANY null -> false.
    assert!(!run_bool(StackItem::Null, StackItem::from_i64(1), OpCode::LT, lt));
    assert!(!run_bool(StackItem::from_i64(1), StackItem::Null, OpCode::LT, lt));
    assert!(!run_bool(StackItem::Null, StackItem::Null, OpCode::LE, le));
    assert!(!run_bool(StackItem::from_i64(1), StackItem::Null, OpCode::GT, gt));
    assert!(!run_bool(StackItem::Null, StackItem::Null, OpCode::GE, ge));
    // Non-null comparisons still work.
    assert!(run_bool(
        StackItem::from_i64(1),
        StackItem::from_i64(2),
        OpCode::LT,
        lt
    ));
}

#[test]
fn numeric_equality_faults_on_null_like_csharp() {
    // C# NumEqual/NumNotEqual call GetInteger() directly (no null check); a
    // Null operand faults.
    let mut e = engine_with_stack(vec![StackItem::Null, StackItem::Null]);
    assert!(numequal(&mut e, &instruction(OpCode::NUMEQUAL)).is_err());
    let mut e = engine_with_stack(vec![StackItem::Null, StackItem::from_i64(0)]);
    assert!(numequal(&mut e, &instruction(OpCode::NUMEQUAL)).is_err());
    let mut e = engine_with_stack(vec![StackItem::Null, StackItem::from_i64(0)]);
    assert!(numnotequal(&mut e, &instruction(OpCode::NUMNOTEQUAL)).is_err());
    // Non-null equality still works.
    assert!(run_bool(
        StackItem::from_i64(1),
        StackItem::from_i64(1),
        OpCode::NUMEQUAL,
        numequal
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

#[test]
fn shift_rejects_out_of_i32_operand_like_csharp() {
    // C# reads the shift as `(int)Pop().GetInteger()`. The `(int)BigInteger` cast
    // throws OverflowException for a value outside i32 range — it does NOT
    // truncate — so SHL by 2^32 FAULTS (it is not an identity shift).
    let two_pow_32 = BigInt::from(1u64 << 32);
    let mut engine = engine_with_stack(vec![
        StackItem::from_i64(7),
        StackItem::from_int(two_pow_32),
    ]);
    assert!(
        shl(&mut engine, &instruction(OpCode::SHL)).is_err(),
        "SHL by 2^32 must fault (C# (int) cast throws OverflowException)"
    );
}

#[test]
fn pow_rejects_out_of_i32_exponent_like_csharp() {
    // C# casts the exponent with `(int)`, which throws OverflowException for a
    // value outside i32 range — POW by 2^32 faults, it does not collapse to ^0.
    let two_pow_32 = BigInt::from(1u64 << 32);
    let mut engine = engine_with_stack(vec![
        StackItem::from_i64(5),
        StackItem::from_int(two_pow_32),
    ]);
    assert!(
        pow(&mut engine, &instruction(OpCode::POW)).is_err(),
        "POW with a 2^32 exponent must fault (C# (int) cast throws OverflowException)"
    );
}

#[test]
fn shift_faults_on_buffer_operand_like_csharp() {
    // C# `(int)Pop().GetInteger()` faults on a Buffer (no GetInteger override).
    let mut engine = engine_with_stack(vec![
        StackItem::from_i64(1),
        StackItem::from_buffer(vec![0x01]),
    ]);
    assert!(
        shl(&mut engine, &instruction(OpCode::SHL)).is_err(),
        "SHL with a Buffer shift operand must fault"
    );
}

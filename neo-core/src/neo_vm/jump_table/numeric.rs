//! Numeric operations for the Neo Virtual Machine.

use crate::neo_vm::error::{VmError, VmResult};
use crate::neo_vm::execution_context::ExecutionContext;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::jump_table::{register_jump_handlers, JumpTable};
use crate::neo_vm::stack_item::StackItem;
use neo_vm_rs::semantics::{arithmetic, comparison};
use neo_vm_rs::{Instruction, OpCode, StackValue};
use num_traits::ToPrimitive;

#[inline]
fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

#[inline]
fn semantics_error(error: String) -> VmError {
    VmError::invalid_operation_msg(error)
}

#[inline]
fn value_from_stack_item(item: StackItem) -> VmResult<StackValue> {
    match item {
        StackItem::Buffer(buffer) => Ok(StackValue::ByteString(buffer.data())),
        item => StackValue::try_from(item),
    }
}

#[inline]
fn push_stack_value(ctx: &mut ExecutionContext, value: StackValue) -> VmResult<()> {
    ctx.push(StackItem::try_from(value)?)
}

fn unary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = value_from_stack_item(ctx.pop()?)?;
    let result = op(value).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn binary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = value_from_stack_item(ctx.pop()?)?;
    let left = value_from_stack_item(ctx.pop()?)?;
    let result = op(left, right).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn ternary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let third = value_from_stack_item(ctx.pop()?)?;
    let second = value_from_stack_item(ctx.pop()?)?;
    let first = value_from_stack_item(ctx.pop()?)?;
    let result = op(first, second, third).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

/// Registers the numeric operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    register_jump_handlers![
        jump_table;
        OpCode::INC => inc,
        OpCode::DEC => dec,
        OpCode::SIGN => sign,
        OpCode::NEGATE => negate,
        OpCode::ABS => abs,
        OpCode::SQRT => sqrt,
        OpCode::NOT => not,
        OpCode::NZ => nz,
        OpCode::ADD => add,
        OpCode::SUB => sub,
        OpCode::MUL => mul,
        OpCode::DIV => div,
        OpCode::MOD => modulo,
        OpCode::POW => pow,
        OpCode::SHL => shl,
        OpCode::SHR => shr,
        OpCode::MIN => min,
        OpCode::MAX => max,
        OpCode::LT => lt,
        OpCode::LE => le,
        OpCode::GT => gt,
        OpCode::GE => ge,
        OpCode::NUMEQUAL => numequal,
        OpCode::NUMNOTEQUAL => numnotequal,
        OpCode::WITHIN => within,
        OpCode::BOOLAND => booland,
        OpCode::BOOLOR => boolor,
        OpCode::MODMUL => modmul,
        OpCode::MODPOW => modpow,
    ];
}

#[inline]
fn inc(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::inc_value)
}

#[inline]
fn dec(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::dec_value)
}

fn sign(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::sign_value)
}

fn negate(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::negate_value)
}

fn abs(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::abs_value)
}

fn sqrt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, arithmetic::sqrt_value)
}

fn not(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = value_from_stack_item(ctx.pop()?)?;
    let result = comparison::not_value(&value).map_err(semantics_error)?;
    ctx.push(StackItem::from_bool(result))
}

fn nz(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = value_from_stack_item(ctx.pop()?)?;
    let result = comparison::nz_value(&value).map_err(semantics_error)?;
    ctx.push(StackItem::from_bool(result))
}

#[inline]
fn add(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::add_values)
}

#[inline]
fn sub(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::sub_values)
}

#[inline]
fn mul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::mul_values)
}

fn div(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::div_values)
}

fn modulo(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::modulo_values)
}

fn pow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let exponent_item = ctx.pop()?;
    let base = value_from_stack_item(ctx.pop()?)?;
    let exponent_i32 = exponent_item
        .as_int()?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Exponent too large"))?;
    limits
        .assert_shift(exponent_i32)
        .map_err(VmError::invalid_operation_msg)?;
    let exponent = value_from_stack_item(exponent_item)?;
    let result = arithmetic::pow_values(base, exponent).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn shl(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift(engine, arithmetic::shl_value, "Shift amount too large")
}

fn shr(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift(engine, arithmetic::shr_value, "Shift amount too large")
}

fn shift(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, i64) -> Result<StackValue, String>,
    overflow_message: &'static str,
) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let shift_item = ctx.pop()?;
    let value = value_from_stack_item(ctx.pop()?)?;
    let shift_i32 = shift_item
        .as_int()?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg(overflow_message))?;
    limits
        .assert_shift(shift_i32)
        .map_err(VmError::invalid_operation_msg)?;
    let result = op(value, i64::from(shift_i32)).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn min(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::min_values)
}

fn max(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, arithmetic::max_values)
}

fn within(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let upper = value_from_stack_item(ctx.pop()?)?;
    let lower = value_from_stack_item(ctx.pop()?)?;
    let value = value_from_stack_item(ctx.pop()?)?;
    let result = arithmetic::within_values(value, lower, upper).map_err(semantics_error)?;
    ctx.push(StackItem::from_bool(result))
}

fn compare_with_null(
    engine: &mut ExecutionEngine,
    null_null: bool,
    null_other: bool,
    other_null: bool,
    op: fn(&StackValue, &StackValue) -> Result<bool, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let right = ctx.pop()?;
    let left = ctx.pop()?;

    let result = match (&left, &right) {
        (StackItem::Null, StackItem::Null) => null_null,
        (StackItem::Null, _) => null_other,
        (_, StackItem::Null) => other_null,
        _ => {
            let left = value_from_stack_item(left)?;
            let right = value_from_stack_item(right)?;
            op(&left, &right).map_err(semantics_error)?
        }
    };
    ctx.push(StackItem::from_bool(result))
}

#[inline]
fn lt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, false, true, false, comparison::less_than_values)
}

#[inline]
fn le(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, true, true, false, comparison::less_or_equal_values)
}

#[inline]
fn gt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, false, false, true, comparison::greater_than_values)
}

#[inline]
fn ge(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(
        engine,
        true,
        false,
        true,
        comparison::greater_or_equal_values,
    )
}

fn numequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, true, false, comparison::num_equal_values)
}

fn numnotequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, false, true, comparison::num_not_equal_values)
}

fn numeric_equality(
    engine: &mut ExecutionEngine,
    null_null: bool,
    null_other: bool,
    op: fn(&StackValue, &StackValue) -> Result<bool, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let right = ctx.pop()?;
    let left = ctx.pop()?;

    let result = match (&left, &right) {
        (StackItem::Null, StackItem::Null) => null_null,
        (StackItem::Null, _) | (_, StackItem::Null) => null_other,
        _ => {
            let left = value_from_stack_item(left)?;
            let right = value_from_stack_item(right)?;
            op(&left, &right).map_err(semantics_error)?
        }
    };
    ctx.push(StackItem::from_bool(result))
}

fn booland(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = ctx.pop()?.as_bool()?;
    let left = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(comparison::bool_and(left, right)))
}

fn boolor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = ctx.pop()?.as_bool()?;
    let left = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(comparison::bool_or(left, right)))
}

fn modmul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    ternary_numeric(engine, arithmetic::modmul_values)
}

fn modpow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    ternary_numeric(engine, arithmetic::modpow_values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::neo_vm::script::Script;
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
}

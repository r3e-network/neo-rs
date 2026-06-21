//! Numeric operations for the Neo Virtual Machine.

use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::stack_item::StackItem;
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
        // C# numeric/comparison/arithmetic opcodes coerce operands via
        // StackItem.GetInteger(). Buffer (not a PrimitiveType, no GetInteger
        // override) and Null both hit the base `GetInteger() => throw
        // InvalidCastException` and FAULT — they are NOT numeric operands.
        // (The CONVERT opcode uses a separate ConvertTo path, unaffected.)
        StackItem::Buffer(_) => Err(VmError::invalid_type_simple(
            "Buffer is not a valid numeric operand (C# GetInteger faults)",
        )),
        StackItem::Null => Err(VmError::invalid_type_simple(
            "Null is not a valid numeric operand (C# GetInteger faults)",
        )),
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

/// Pre-`HF_Gorgon` (neo-vm#567) vulnerable SHL. Unlike the fixed [`shift`], it
/// does NOT pop/validate the value operand when the shift is zero — it returns
/// with the value still on the stack (C# `ApplicationEngine.VulnerableSHL`).
pub(crate) fn shl_vulnerable(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift_vulnerable(engine, arithmetic::shl_value, "Shift amount too large")
}

/// Pre-`HF_Gorgon` (neo-vm#567) vulnerable SHR (see [`shl_vulnerable`]).
pub(crate) fn shr_vulnerable(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift_vulnerable(engine, arithmetic::shr_value, "Shift amount too large")
}

fn shift_vulnerable(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, i64) -> Result<StackValue, String>,
    overflow_message: &'static str,
) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    // C# VulnerableSHL/SHR: pop shift, assert, and on zero shift return WITHOUT
    // popping the value operand (so a non-primitive value is never validated and
    // stays on the stack) — the divergence from the fixed handler.
    let shift_i32 = ctx
        .pop()?
        .as_int()?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg(overflow_message))?;
    limits
        .assert_shift(shift_i32)
        .map_err(VmError::invalid_operation_msg)?;
    if shift_i32 == 0 {
        return Ok(());
    }
    let value = value_from_stack_item(ctx.pop()?)?;
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

/// C# `JumpTable.Numeric` Lt/Le/Gt/Ge: `if (x1.IsNull || x2.IsNull) Push(false)`
/// — ANY null operand pushes false; otherwise compare `GetInteger()` of each
/// (which faults on Buffer / non-numeric via `value_from_stack_item`).
fn compare(
    engine: &mut ExecutionEngine,
    op: fn(&StackValue, &StackValue) -> Result<bool, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let right = ctx.pop()?;
    let left = ctx.pop()?;

    let result = if matches!(left, StackItem::Null) || matches!(right, StackItem::Null) {
        false
    } else {
        let left = value_from_stack_item(left)?;
        let right = value_from_stack_item(right)?;
        op(&left, &right).map_err(semantics_error)?
    };
    ctx.push(StackItem::from_bool(result))
}

#[inline]
fn lt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare(engine, comparison::less_than_values)
}

#[inline]
fn le(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare(engine, comparison::less_or_equal_values)
}

#[inline]
fn gt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare(engine, comparison::greater_than_values)
}

#[inline]
fn ge(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare(engine, comparison::greater_or_equal_values)
}

fn numequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, comparison::num_equal_values)
}

fn numnotequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, comparison::num_not_equal_values)
}

/// C# `JumpTable.Numeric` NumEqual/NumNotEqual: `Pop().GetInteger()` on each with
/// NO null check — a Null (or Buffer) operand FAULTS via `GetInteger`.
fn numeric_equality(
    engine: &mut ExecutionEngine,
    op: fn(&StackValue, &StackValue) -> Result<bool, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let right = ctx.pop()?;
    let left = ctx.pop()?;

    let left = value_from_stack_item(left)?;
    let right = value_from_stack_item(right)?;
    let result = op(&left, &right).map_err(semantics_error)?;
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
#[path = "../tests/jump_table/numeric.rs"]
mod tests;

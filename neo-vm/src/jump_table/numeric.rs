//! Numeric operations for the Neo Virtual Machine.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{
    JumpTable, numeric_operand, push_stack_value, register_jump_handlers, require_context,
    semantics_error,
};
use crate::stack_item::StackItem;
use neo_vm_rs::semantics::{arithmetic, comparison};
use neo_vm_rs::{Instruction, OpCode, StackValue};
use num_traits::ToPrimitive;

/// C# `int shift = (int)engine.Pop().GetInteger()` for a shift/exponent operand.
///
/// `GetInteger()` faults on a non-integer operand (a `Buffer` is not a
/// `PrimitiveType`, and `Null`), and the `(int)BigInteger` cast throws
/// `OverflowException` — it does NOT truncate — when the value is outside the
/// `i32` range. So a `Buffer`/`Null` operand and an out-of-`i32` value both
/// fault, exactly as the reference VM does (`AssertShift` then narrows the
/// in-range value to `[0, MaxShift]`).
fn shift_operand_to_i32(item: StackItem) -> VmResult<i32> {
    super::get_integer(item)?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount out of Int32 range"))
}

fn unary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = numeric_operand(ctx.pop()?)?;
    let result = op(value).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn binary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = numeric_operand(ctx.pop()?)?;
    let left = numeric_operand(ctx.pop()?)?;
    let result = op(left, right).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn ternary_numeric(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let third = numeric_operand(ctx.pop()?)?;
    let second = numeric_operand(ctx.pop()?)?;
    let first = numeric_operand(ctx.pop()?)?;
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
    // C# `Not` reads the operand via `GetBoolean()` (JumpTable.Numeric.cs:271-274),
    // which never faults on type: Null=>false, Buffer/Array/Struct/Map/Pointer/
    // Interop=>true, ByteString size-checked. Do NOT route through `numeric_operand`
    // (the GetInteger path) — that would wrongly fault on a Buffer/Null operand.
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(!value))
}

fn nz(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = numeric_operand(ctx.pop()?)?;
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
    // C# Pow: `var exponent = (int)Pop().GetInteger(); AssertShift(exponent);
    // var value = Pop().GetInteger(); Push(BigInteger.Pow(value, exponent))`.
    // The exponent is the TRUNCATED int (not the full BigInteger), so the actual
    // power uses the truncated value — match that here.
    let exponent_i32 = shift_operand_to_i32(ctx.pop()?)?;
    limits
        .assert_shift(exponent_i32)
        .map_err(VmError::invalid_operation_msg)?;
    let base = numeric_operand(ctx.pop()?)?;
    let exponent = numeric_operand(StackItem::from_int(exponent_i32))?;
    let result = arithmetic::pow_values(base, exponent).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn shl(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift(engine, arithmetic::shl_value)
}

fn shr(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    shift(engine, arithmetic::shr_value)
}

/// SHL/SHR. Neo.VM **3.10.0** `Shl`/`Shr` pop the shift, narrow it to `int`,
/// `AssertShift`, then UNCONDITIONALLY pop the value operand and `GetInteger()` it
/// (`BigInteger integer = engine.Pop().GetInteger(); engine.Push(integer << num)`).
/// So a zero shift still pops + integer-coerces the value: a non-integer operand
/// FAULTS, and an integer-coercible operand (Boolean/ByteString/…) is re-pushed as
/// an Integer. This is a flat Neo.VM 3.9.0→3.10.0 change (3.9.0 had
/// `if (num != 0) { … }`, leaving the operand untouched on a zero shift) — verified
/// by decompiling both `Neo.VM.dll` versions — and is NOT hardfork-gated, so the
/// early-return must not be reintroduced for any protocol height.
fn shift(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, i64) -> Result<StackValue, String>,
) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let shift_i32 = shift_operand_to_i32(ctx.pop()?)?;
    limits
        .assert_shift(shift_i32)
        .map_err(VmError::invalid_operation_msg)?;
    // C# 3.10.0 `BigInteger integer = Pop().GetInteger()` faults on a Buffer/Null
    // operand and integer-coerces a Boolean/ByteString/Integer; `Push(integer <<
    // num)` then re-pushes an Integer, even for a zero shift. Coerce to an Integer
    // operand first so the shift op yields an Integer (the semantics layer's own
    // zero-shift shortcut would otherwise preserve the original Boolean/ByteString
    // type).
    let integer = super::get_integer(ctx.pop()?)?;
    let value = numeric_operand(StackItem::from_int(integer))?;
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
    let upper = numeric_operand(ctx.pop()?)?;
    let lower = numeric_operand(ctx.pop()?)?;
    let value = numeric_operand(ctx.pop()?)?;
    let result = arithmetic::within_values(value, lower, upper).map_err(semantics_error)?;
    ctx.push(StackItem::from_bool(result))
}

/// C# `JumpTable.Numeric` Lt/Le/Gt/Ge: `if (x1.IsNull || x2.IsNull) Push(false)`
/// — ANY null operand pushes false; otherwise compare `GetInteger()` of each
/// (which faults on Buffer / non-numeric via `numeric_operand`).
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
        let left = numeric_operand(left)?;
        let right = numeric_operand(right)?;
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

    let left = numeric_operand(left)?;
    let right = numeric_operand(right)?;
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

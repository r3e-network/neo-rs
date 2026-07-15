//! Numeric operations for the Neo Virtual Machine.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::shared::push_integer;
use crate::jump_table::{
    JumpTable, get_integer, get_vm_integer, register_jump_handlers, require_context,
};
use crate::stack_item::{StackItem, VmInteger};
use crate::{Instruction, OpCode};
use num_bigint::{BigInt, Sign};
use num_traits::{One, ToPrimitive, Zero};

/// C# `int shift = (int)engine.Pop().GetInteger()` for a shift/exponent operand.
///
/// `GetInteger()` faults on a non-integer operand (a `Buffer` is not a
/// `PrimitiveType`, and `Null`), and the `(int)BigInteger` cast throws
/// `OverflowException` — it does NOT truncate — when the value is outside the
/// `i32` range. So a `Buffer`/`Null` operand and an out-of-`i32` value both
/// fault, exactly as the reference VM does (`AssertShift` then narrows the
/// in-range value to `[0, MaxShift]`).
fn shift_operand_to_i32(item: StackItem) -> VmResult<i32> {
    get_integer(item)?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount out of Int32 range"))
}

fn unary_numeric<S, F>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    op: F,
) -> VmResult<()>
where
    F: FnOnce(BigInt) -> VmResult<BigInt>,
{
    let ctx = require_context(engine)?;
    let value = get_integer(ctx.pop()?)?;
    let result = op(value)?;
    push_integer(ctx, result, overflow_message)
}

fn binary_numeric<S, F>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    op: F,
) -> VmResult<()>
where
    F: FnOnce(BigInt, BigInt) -> VmResult<BigInt>,
{
    let ctx = require_context(engine)?;
    let right = get_integer(ctx.pop()?)?;
    let left = get_integer(ctx.pop()?)?;
    let result = op(left, right)?;
    push_integer(ctx, result, overflow_message)
}

enum ArithmeticOperand {
    Small(i64),
    Big(BigInt),
}

impl ArithmeticOperand {
    #[inline]
    fn into_bigint(self) -> BigInt {
        match self {
            Self::Small(value) => BigInt::from(value),
            Self::Big(value) => value,
        }
    }
}

#[inline]
fn arithmetic_operand(item: StackItem) -> VmResult<ArithmeticOperand> {
    match item {
        StackItem::Integer(VmInteger::Small(value)) => Ok(ArithmeticOperand::Small(value)),
        item => get_integer(item).map(ArithmeticOperand::Big),
    }
}

#[inline]
fn binary_checked_small_numeric<S, FSmall, FBig>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    small_op: FSmall,
    big_op: FBig,
) -> VmResult<()>
where
    FSmall: FnOnce(i64, i64) -> Option<i64>,
    FBig: FnOnce(BigInt, BigInt) -> BigInt,
{
    let ctx = require_context(engine)?;
    let right = arithmetic_operand(ctx.pop()?)?;
    let left = arithmetic_operand(ctx.pop()?)?;

    match (left, right) {
        (ArithmeticOperand::Small(left), ArithmeticOperand::Small(right)) => {
            if let Some(result) = small_op(left, right) {
                return ctx.push(StackItem::from_i64(result));
            }
            push_integer(
                ctx,
                big_op(BigInt::from(left), BigInt::from(right)),
                overflow_message,
            )
        }
        (left, right) => push_integer(
            ctx,
            big_op(left.into_bigint(), right.into_bigint()),
            overflow_message,
        ),
    }
}

fn ternary_numeric<S, F>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    op: F,
) -> VmResult<()>
where
    F: FnOnce(BigInt, BigInt, BigInt) -> VmResult<BigInt>,
{
    let ctx = require_context(engine)?;
    let third = get_integer(ctx.pop()?)?;
    let second = get_integer(ctx.pop()?)?;
    let first = get_integer(ctx.pop()?)?;
    let result = op(first, second, third)?;
    push_integer(ctx, result, overflow_message)
}

#[inline]
fn arithmetic_fault(message: &'static str) -> VmError {
    VmError::invalid_operation_msg(message)
}

fn modular_inverse(value: BigInt, modulus: &BigInt) -> VmResult<BigInt> {
    if value <= BigInt::zero() {
        return Err(arithmetic_fault("value has no modular inverse"));
    }
    if modulus <= &BigInt::one() {
        return Err(arithmetic_fault("invalid modulus for modular inverse"));
    }
    value
        .modinv(modulus)
        .ok_or_else(|| arithmetic_fault("value is not invertible for MODPOW"))
}

/// Matches .NET `BigInteger.ModPow`, whose remainder keeps the dividend's
/// sign. `num_bigint::BigInt::modpow` uses floor-mod semantics instead, so it
/// produces different results for negative bases or moduli.
fn modular_power(mut base: BigInt, mut exponent: BigInt, modulus: &BigInt) -> VmResult<BigInt> {
    if modulus.is_zero() {
        return Err(arithmetic_fault("division by zero for MODPOW"));
    }
    if exponent == BigInt::from(-1) {
        return modular_inverse(base, modulus);
    }
    if exponent.sign() == Sign::Minus {
        return Err(arithmetic_fault("negative exponent for MODPOW"));
    }

    let mut result = BigInt::one() % modulus;
    base %= modulus;
    while !exponent.is_zero() {
        if (&exponent & BigInt::one()) == BigInt::one() {
            result = (result * &base) % modulus;
        }
        exponent >>= 1usize;
        if !exponent.is_zero() {
            base = (&base * &base) % modulus;
        }
    }
    Ok(result)
}

/// Registers the numeric operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
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
fn inc<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for INC", |value| {
        Ok(value + BigInt::one())
    })
}

#[inline]
fn dec<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for DEC", |value| {
        Ok(value - BigInt::one())
    })
}

fn sign<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for SIGN", |value| {
        Ok(BigInt::from(match value.sign() {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        }))
    })
}

fn negate<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for NEGATE", |value| Ok(-value))
}

fn abs<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for ABS", |value| {
        Ok(if value.sign() == Sign::Minus {
            -value
        } else {
            value
        })
    })
}

fn sqrt<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    unary_numeric(engine, "integer overflow for SQRT", |value| {
        if value.sign() == Sign::Minus {
            return Err(arithmetic_fault("negative value for SQRT"));
        }
        Ok(value.sqrt())
    })
}

fn not<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    // C# `Not` reads the operand via `GetBoolean()` (JumpTable.Numeric.cs:271-274),
    // which never faults on type: Null=>false, Buffer/Array/Struct/Map/Pointer/
    // Interop=>true, ByteString size-checked. Do NOT route through `get_integer`
    // (the GetInteger path) — that would wrongly fault on a Buffer/Null operand.
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(!value))
}

fn nz<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = get_vm_integer(ctx.pop()?)?;
    ctx.push(StackItem::from_bool(!value.is_zero()))
}

#[inline]
fn add<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_checked_small_numeric(
        engine,
        "integer overflow for ADD",
        i64::checked_add,
        |left, right| left + right,
    )
}

#[inline]
fn sub<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_checked_small_numeric(
        engine,
        "integer overflow for SUB",
        i64::checked_sub,
        |left, right| left - right,
    )
}

#[inline]
fn mul<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_checked_small_numeric(
        engine,
        "integer overflow for MUL",
        i64::checked_mul,
        |left, right| left * right,
    )
}

fn div<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, "integer overflow for DIV", |left, right| {
        if right.is_zero() {
            return Err(arithmetic_fault("division by zero for DIV"));
        }
        Ok(left / right)
    })
}

fn modulo<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, "integer overflow for MOD", |left, right| {
        if right.is_zero() {
            return Err(arithmetic_fault("division by zero for MOD"));
        }
        Ok(left % right)
    })
}

fn pow<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    // C# Pow: `var exponent = (int)Pop().GetInteger(); AssertShift(exponent);
    // var value = Pop().GetInteger(); Push(BigInteger.Pow(value, exponent))`.
    // The checked C# cast faults instead of truncating when the value is outside
    // the Int32 range.
    let exponent_i32 = shift_operand_to_i32(ctx.pop()?)?;
    limits
        .assert_shift(exponent_i32)
        .map_err(VmError::invalid_operation_msg)?;
    let base = get_integer(ctx.pop()?)?;
    let result = base.pow(exponent_i32 as u32);
    push_integer(ctx, result, "integer overflow for POW")
}

fn shl<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    shift(engine, "integer overflow for SHL", true, |value, shift| {
        value << shift
    })
}

fn shr<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    shift(engine, "integer overflow for SHR", true, |value, shift| {
        value >> shift
    })
}

/// Pre-`HF_Gorgon` SHL implementation from C# `ApplicationEngine.VulnerableSHL`.
/// A zero shift consumes only the shift operand and leaves the value untouched.
pub(crate) fn vulnerable_shl<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    shift(engine, "integer overflow for SHL", false, |value, shift| {
        value << shift
    })
}

/// Pre-`HF_Gorgon` SHR implementation from C# `ApplicationEngine.VulnerableSHR`.
/// A zero shift consumes only the shift operand and leaves the value untouched.
pub(crate) fn vulnerable_shr<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    shift(engine, "integer overflow for SHR", false, |value, shift| {
        value >> shift
    })
}

/// SHL/SHR shared implementation. The default v3.10.1 handlers always pop and
/// integer-coerce the value. C# selects the vulnerable early-return behavior
/// before `HF_Gorgon` to preserve historical block execution.
fn shift<S, F>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    pop_value_on_zero: bool,
    op: F,
) -> VmResult<()>
where
    F: FnOnce(BigInt, usize) -> BigInt,
{
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let shift_i32 = shift_operand_to_i32(ctx.pop()?)?;
    limits
        .assert_shift(shift_i32)
        .map_err(VmError::invalid_operation_msg)?;
    if shift_i32 == 0 && !pop_value_on_zero {
        return Ok(());
    }
    let value = get_integer(ctx.pop()?)?;
    let result = op(value, shift_i32 as usize);
    push_integer(ctx, result, overflow_message)
}

fn min<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, "integer overflow for MIN", |left, right| {
        Ok(left.min(right))
    })
}

fn max<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_numeric(engine, "integer overflow for MAX", |left, right| {
        Ok(left.max(right))
    })
}

fn within<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let upper = get_vm_integer(ctx.pop()?)?;
    let lower = get_vm_integer(ctx.pop()?)?;
    let value = get_vm_integer(ctx.pop()?)?;
    let result = lower <= value && value < upper;
    ctx.push(StackItem::from_bool(result))
}

/// C# `JumpTable.Numeric` Lt/Le/Gt/Ge: `if (x1.IsNull || x2.IsNull) Push(false)`
/// — ANY null operand pushes false; otherwise compare `GetInteger()` of each
/// (which faults on Buffer / non-numeric via `get_vm_integer`).
fn compare<S, F>(engine: &mut ExecutionEngine<S>, op: F) -> VmResult<()>
where
    F: FnOnce(&VmInteger, &VmInteger) -> bool,
{
    let ctx = require_context(engine)?;
    let right = ctx.pop()?;
    let left = ctx.pop()?;

    let result = if matches!(left, StackItem::Null) || matches!(right, StackItem::Null) {
        false
    } else {
        let left = get_vm_integer(left)?;
        let right = get_vm_integer(right)?;
        op(&left, &right)
    };
    ctx.push(StackItem::from_bool(result))
}

#[inline]
fn lt<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    compare(engine, |left, right| left < right)
}

#[inline]
fn le<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    compare(engine, |left, right| left <= right)
}

#[inline]
fn gt<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    compare(engine, |left, right| left > right)
}

#[inline]
fn ge<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    compare(engine, |left, right| left >= right)
}

fn numequal<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, |left, right| left == right)
}

fn numnotequal<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    numeric_equality(engine, |left, right| left != right)
}

/// C# `JumpTable.Numeric` NumEqual/NumNotEqual: `Pop().GetInteger()` on each with
/// NO null check — a Null (or Buffer) operand FAULTS via `GetInteger`.
fn numeric_equality<S, F>(engine: &mut ExecutionEngine<S>, op: F) -> VmResult<()>
where
    F: FnOnce(&VmInteger, &VmInteger) -> bool,
{
    let ctx = require_context(engine)?;
    let right = get_vm_integer(ctx.pop()?)?;
    let left = get_vm_integer(ctx.pop()?)?;
    let result = op(&left, &right);
    ctx.push(StackItem::from_bool(result))
}

fn booland<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = ctx.pop()?.as_bool()?;
    let left = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(left && right))
}

fn boolor<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = ctx.pop()?.as_bool()?;
    let left = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(left || right))
}

fn modmul<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    ternary_numeric(
        engine,
        "integer overflow for MODMUL",
        |left, right, modulus| {
            if modulus.is_zero() {
                return Err(arithmetic_fault("division by zero for MODMUL"));
            }
            Ok((left * right) % modulus)
        },
    )
}

fn modpow<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    ternary_numeric(
        engine,
        "integer overflow for MODPOW",
        |base, exponent, modulus| modular_power(base, exponent, &modulus),
    )
}

#[cfg(test)]
#[path = "../../tests/jump_table/numeric.rs"]
mod tests;

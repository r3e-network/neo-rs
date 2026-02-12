//! Numeric operations for the Neo Virtual Machine.
//!
//! This module provides the numeric operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::StackItem;
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

/// Maximum size for `BigInt` results in bytes (256 bits = 32 bytes)
const MAX_BIGINT_SIZE: usize = 32;

/// Helper to get current context or return error.
#[inline]
fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Checks if a `BigInt` value exceeds the maximum allowed size.
///
/// Uses `BigInt::bits()` to compute the byte length without allocating a `Vec<u8>`.
/// `bits()` returns the number of bits needed to represent the magnitude, so we
/// add 1 for the sign bit and round up to whole bytes: `(bits + 8) / 8`.
/// For zero, `bits()` returns 0 and the signed encoding is a single `0x00` byte.
#[inline]
fn check_bigint_size(value: &BigInt) -> VmResult<()> {
    let bits = value.bits();
    // Zero encodes as a single byte in signed two's-complement representation.
    // For non-zero values: need `bits` magnitude bits + 1 sign bit, rounded up to bytes.
    let byte_len = if bits == 0 { 1 } else { (bits as usize + 8) / 8 };
    if byte_len > MAX_BIGINT_SIZE {
        return Err(VmError::invalid_operation_msg(format!(
            "BigInt size {byte_len} bytes exceeds maximum {MAX_BIGINT_SIZE} bytes"
        )));
    }
    Ok(())
}

/// Registers the numeric operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    // Unary operations
    jump_table.register(OpCode::INC, inc);
    jump_table.register(OpCode::DEC, dec);
    jump_table.register(OpCode::SIGN, sign);
    jump_table.register(OpCode::NEGATE, negate);
    jump_table.register(OpCode::ABS, abs);
    jump_table.register(OpCode::SQRT, sqrt);
    jump_table.register(OpCode::NOT, not);
    jump_table.register(OpCode::NZ, nz);

    // Binary arithmetic
    jump_table.register(OpCode::ADD, add);
    jump_table.register(OpCode::SUB, sub);
    jump_table.register(OpCode::MUL, mul);
    jump_table.register(OpCode::DIV, div);
    jump_table.register(OpCode::MOD, modulo);
    jump_table.register(OpCode::POW, pow);
    jump_table.register(OpCode::SHL, shl);
    jump_table.register(OpCode::SHR, shr);
    jump_table.register(OpCode::MIN, min);
    jump_table.register(OpCode::MAX, max);

    // Comparison operations
    jump_table.register(OpCode::LT, lt);
    jump_table.register(OpCode::LE, le);
    jump_table.register(OpCode::GT, gt);
    jump_table.register(OpCode::GE, ge);
    jump_table.register(OpCode::NUMEQUAL, numequal);
    jump_table.register(OpCode::NUMNOTEQUAL, numnotequal);
    jump_table.register(OpCode::WITHIN, within);

    // Logical operations
    jump_table.register(OpCode::BOOLAND, booland);
    jump_table.register(OpCode::BOOLOR, boolor);

    // Advanced numeric operations
    jump_table.register(OpCode::MODMUL, modmul);
    jump_table.register(OpCode::MODPOW, modpow);
}

// ============================================================================
// Unary Operations
// ============================================================================

fn inc(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    let result = value + BigInt::one();
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn dec(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    let result = value - BigInt::one();
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn sign(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    let result = if value.is_zero() {
        BigInt::zero()
    } else if value.is_positive() {
        BigInt::one()
    } else {
        -BigInt::one()
    };
    ctx.push(StackItem::from_int(result))
}

fn negate(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    let result = -value;
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn abs(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    let result = value.abs();
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn sqrt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    if value.is_negative() {
        return Err(VmError::invalid_operation_msg(
            "Square root of negative number",
        ));
    }
    let result = integer_sqrt(&value);
    ctx.push(StackItem::from_int(result))
}

fn not(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?;
    let result = match value {
        StackItem::Boolean(b) => !b,
        StackItem::Integer(i) => i.is_zero(),
        StackItem::Null => true,
        _ => false,
    };
    ctx.push(StackItem::from_bool(result))
}

fn nz(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_bool(!value.is_zero()))
}

// ============================================================================
// Binary Arithmetic Operations
// ============================================================================

fn add(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?;
    let a = ctx.pop()?;

    let result = match (a, b) {
        (StackItem::Integer(a), StackItem::Integer(b)) => {
            let sum = &a + &b;
            check_bigint_size(&sum)?;
            StackItem::from_int(sum)
        }
        (StackItem::ByteString(a), StackItem::ByteString(b)) => {
            let mut result = a;
            result.extend_from_slice(&b);
            StackItem::from_byte_string(result)
        }
        (StackItem::Buffer(a), StackItem::Buffer(b)) => {
            a.extend_from_slice(&b.data());
            StackItem::Buffer(a)
        }
        (a, b) => {
            let sum = a.as_int()? + b.as_int()?;
            check_bigint_size(&sum)?;
            StackItem::from_int(sum)
        }
    };
    ctx.push(result)
}

fn sub(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    let result = a - b;
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn mul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    let result = a * b;
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn div(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }
    ctx.push(StackItem::from_int(a / b))
}

fn modulo(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }
    ctx.push(StackItem::from_int(a % b))
}

fn pow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;

    let exponent_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Exponent too large"))?;
    limits.assert_shift(exponent_i32)?;

    let result = a.pow(exponent_i32 as u32);
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn shl(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;

    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        return ctx.push(StackItem::from_int(a));
    }

    let result = a << (shift_i32 as u32);
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn shr(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;

    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        return ctx.push(StackItem::from_int(a));
    }

    let result = a >> (shift_i32 as u32);
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn min(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_int(if a < b { a } else { b }))
}

fn max(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_int(if a > b { a } else { b }))
}

// ============================================================================
// Comparison Operations
// ============================================================================

fn within(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    let x = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_bool(a <= x && x < b))
}

/// Helper for comparison operations with null handling
fn compare_with_null<F>(
    engine: &mut ExecutionEngine,
    null_null: bool,
    null_other: bool,
    other_null: bool,
    cmp: F,
) -> VmResult<()>
where
    F: FnOnce(&BigInt, &BigInt) -> bool,
{
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let b = ctx.pop()?;
    let a = ctx.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => null_null,
        (StackItem::Null, _) => null_other,
        (_, StackItem::Null) => other_null,
        _ => cmp(&a.as_int()?, &b.as_int()?),
    };
    ctx.push(StackItem::from_bool(result))
}

fn lt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, false, true, false, |a, b| a < b)
}

fn le(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, true, true, false, |a, b| a <= b)
}

fn gt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, false, false, true, |a, b| a > b)
}

fn ge(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(engine, true, false, true, |a, b| a >= b)
}

fn numequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let b = ctx.pop()?;
    let a = ctx.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => true,
        (StackItem::Null, _) | (_, StackItem::Null) => false,
        (StackItem::Boolean(a_bool), StackItem::Boolean(b_bool)) => a_bool == b_bool,
        (StackItem::Boolean(a_bool), _) => BigInt::from(i32::from(*a_bool)) == b.as_int()?,
        (_, StackItem::Boolean(b_bool)) => a.as_int()? == BigInt::from(i32::from(*b_bool)),
        _ => a.as_int()? == b.as_int()?,
    };
    ctx.push(StackItem::from_bool(result))
}

fn numnotequal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    if ctx.evaluation_stack().len() < 2 {
        return Err(VmError::insufficient_stack_items_msg(0, 0));
    }
    let b = ctx.pop()?;
    let a = ctx.pop()?;

    let result = match (&a, &b) {
        (StackItem::Null, StackItem::Null) => false,
        (StackItem::Null, _) | (_, StackItem::Null) => true,
        (StackItem::Boolean(a_bool), StackItem::Boolean(b_bool)) => a_bool != b_bool,
        (StackItem::Boolean(a_bool), _) => BigInt::from(i32::from(*a_bool)) != b.as_int()?,
        (_, StackItem::Boolean(b_bool)) => a.as_int()? != BigInt::from(i32::from(*b_bool)),
        _ => a.as_int()? != b.as_int()?,
    };
    ctx.push(StackItem::from_bool(result))
}

// ============================================================================
// Logical Operations
// ============================================================================

fn booland(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_bool()?;
    let a = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(a && b))
}

fn boolor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_bool()?;
    let a = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(a || b))
}

// ============================================================================
// Advanced Numeric Operations
// ============================================================================

fn modmul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let modulus = ctx.pop()?.as_int()?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;

    if modulus.is_zero() {
        return Err(VmError::division_by_zero_msg("division"));
    }

    let result = (a * b) % modulus;
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn modpow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let modulus = ctx.pop()?.as_int()?;
    let exponent = ctx.pop()?.as_int()?;
    let base = ctx.pop()?.as_int()?;

    // Exponent == -1 triggers modular inverse
    if exponent == -BigInt::one() {
        let result = mod_inverse(&base, &modulus)?;
        check_bigint_size(&result)?;
        return ctx.push(StackItem::from_int(result));
    }

    if exponent < -BigInt::one() {
        return Err(VmError::invalid_operation_msg(
            "Exponent less than -1 not supported",
        ));
    }

    if modulus.is_zero() {
        return Err(VmError::division_by_zero_msg("division"));
    }

    if exponent.is_negative() {
        return Err(VmError::invalid_operation_msg(
            "Negative exponent not supported",
        ));
    }

    let result = base.modpow(&exponent, &modulus);
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Integer square root using Newton's method (matches C# BigInteger.Sqrt)
fn integer_sqrt(value: &BigInt) -> BigInt {
    if value <= &BigInt::one() {
        return value.clone();
    }

    let mut x = value.clone();
    let mut y: BigInt = (value + 1) / 2;

    while y < x {
        x = y.clone();
        y = (&x + value / &x) / 2;
    }
    x
}

/// Computes the modular inverse of `value` modulo `modulus`.
fn mod_inverse(value: &BigInt, modulus: &BigInt) -> VmResult<BigInt> {
    if value <= &BigInt::zero() {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires positive value",
        ));
    }
    if modulus < &BigInt::from(2u8) {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires modulus >= 2",
        ));
    }

    let mut r = value.clone();
    let mut old_r = modulus.clone();
    let mut s = BigInt::one();
    let mut old_s = BigInt::zero();

    while r > BigInt::zero() {
        let q = &old_r / &r;
        let new_r = &old_r % &r;
        old_r = r;
        r = new_r;

        let new_s = &old_s - &q * &s;
        old_s = s;
        s = new_s;
    }

    let mut result = old_s % modulus;
    if result.is_negative() {
        result += modulus;
    }

    if (value * &result) % modulus != BigInt::one() {
        return Err(VmError::invalid_operation_msg(
            "No modular inverse exists for the given inputs",
        ));
    }

    Ok(result)
}

//! Numeric operations for the Neo Virtual Machine.
//!
//! This module provides the numeric operation handlers for the Neo VM.

use crate::neo_vm::error::VmError;
use crate::neo_vm::error::VmResult;
use crate::neo_vm::execution_context::ExecutionContext;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::instruction::Instruction;
use crate::neo_vm::jump_table::JumpTable;
use crate::neo_vm::stack_item::StackItem;
use neo_vm_rs::{OpCode, StackValue};
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
    let byte_len = if bits == 0 {
        1
    } else {
        (bits as usize + 8) / 8
    };
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

#[inline]
fn inc(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let result = match value.to_i64() {
        Some(value) if value.checked_add(1).is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::inc_i64(value))
        }
        _ => value + 1,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

#[inline]
fn dec(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let result = match value.to_i64() {
        Some(value) if value.checked_sub(1).is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::dec_i64(value))
        }
        _ => value - 1,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn sign(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let sign_val = if let Some(value) = value.to_i64() {
        neo_vm_rs::semantics::arithmetic::sign_i64(value)
    } else {
        match value.sign() {
            num_bigint::Sign::Minus => -1,
            num_bigint::Sign::NoSign => 0,
            num_bigint::Sign::Plus => 1,
        }
    };
    ctx.push(StackItem::from_int(sign_val))
}

fn negate(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let result = match value.to_i64() {
        Some(value) if value.checked_neg().is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::negate_i64(value))
        }
        _ => -value,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn abs(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let result = match value.to_i64() {
        Some(value) if value.checked_abs().is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::abs_i64(value))
        }
        _ => value.abs(),
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn sqrt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    if value.is_negative() {
        return Err(VmError::invalid_operation_msg(
            "Square root of negative number",
        ));
    }
    let result = if let Some(value) = value.to_i64() {
        BigInt::from(
            neo_vm_rs::semantics::arithmetic::sqrt_i64(value)
                .map_err(VmError::invalid_operation_msg)?,
        )
    } else {
        integer_sqrt(&value)
    };
    ctx.push(StackItem::from_int(result))
}

fn not(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let x = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(
        neo_vm_rs::semantics::comparison::bool_not(x),
    ))
}

fn nz(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?.into_int()?;
    let stack_value = StackValue::BigInteger(value.to_signed_bytes_le());
    ctx.push(StackItem::from_bool(neo_vm_rs::semantics::comparison::nz(
        &stack_value,
    )))
}

// ============================================================================
// Binary Arithmetic Operations
// ============================================================================

#[inline]
fn add(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) if a.checked_add(b).is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::add_i64(a, b))
        }
        _ => a + b,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

#[inline]
fn sub(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) if a.checked_sub(b).is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::sub_i64(a, b))
        }
        _ => a - b,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

#[inline]
fn mul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) if a.checked_mul(b).is_some() => {
            BigInt::from(neo_vm_rs::semantics::arithmetic::mul_i64(a, b))
        }
        _ => a * b,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn div(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) if a.checked_div(b).is_some() => BigInt::from(
            neo_vm_rs::semantics::arithmetic::div_i64(a, b)
                .map_err(VmError::invalid_operation_msg)?,
        ),
        _ => a / b,
    };
    ctx.push(StackItem::from_int(result))
}

fn modulo(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    if b.is_zero() {
        return Err(VmError::invalid_operation_msg("Division by zero"));
    }
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) if a.checked_rem(b).is_some() => BigInt::from(
            neo_vm_rs::semantics::arithmetic::modulo_i64(a, b)
                .map_err(VmError::invalid_operation_msg)?,
        ),
        _ => a % b,
    };
    ctx.push(StackItem::from_int(result))
}

fn pow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;

    let exponent_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Exponent too large"))?;
    limits.assert_shift(exponent_i32)?;

    let result = match (a.to_i64(), b.to_i64()) {
        (Some(base), Some(exponent))
            if (0..=63).contains(&exponent) && base.checked_pow(exponent as u32).is_some() =>
        {
            BigInt::from(
                neo_vm_rs::semantics::arithmetic::pow_i64(base, exponent)
                    .map_err(VmError::invalid_operation_msg)?,
            )
        }
        _ => a.pow(exponent_i32 as u32),
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn shl(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;

    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        return ctx.push(StackItem::from_int(a));
    }

    let result = if shift_i32 < 64 {
        let result = a.clone() << (shift_i32 as u32);
        match (a.to_i64(), result.to_i64()) {
            (Some(value), Some(_)) => BigInt::from(
                neo_vm_rs::semantics::arithmetic::shl_i64(value, i64::from(shift_i32))
                    .map_err(VmError::invalid_operation_msg)?,
            ),
            _ => result,
        }
    } else {
        a << (shift_i32 as u32)
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn shr(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;

    let shift_i32 = b
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Shift amount too large"))?;
    limits.assert_shift(shift_i32)?;

    if shift_i32 == 0 {
        return ctx.push(StackItem::from_int(a));
    }

    let result = if shift_i32 < 64 {
        let result = a.clone() >> (shift_i32 as u32);
        match (a.to_i64(), result.to_i64()) {
            (Some(value), Some(_)) => BigInt::from(
                neo_vm_rs::semantics::arithmetic::shr_i64(value, i64::from(shift_i32))
                    .map_err(VmError::invalid_operation_msg)?,
            ),
            _ => result,
        }
    } else {
        a >> (shift_i32 as u32)
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn min(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) => StackItem::from_int(neo_vm_rs::semantics::arithmetic::min_i64(a, b)),
        _ => StackItem::from_int(if a < b { a } else { b }),
    };
    ctx.push(result)
}

fn max(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) => StackItem::from_int(neo_vm_rs::semantics::arithmetic::max_i64(a, b)),
        _ => StackItem::from_int(if a > b { a } else { b }),
    };
    ctx.push(result)
}

// ============================================================================
// Comparison Operations
// ============================================================================

fn within(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let x = ctx.pop()?.into_int()?;
    let result = match (x.to_i64(), a.to_i64(), b.to_i64()) {
        (Some(x), Some(a), Some(b)) => neo_vm_rs::semantics::arithmetic::within_i64(x, a, b),
        _ => a <= x && x < b,
    };
    ctx.push(StackItem::from_bool(result))
}

/// Helper for comparison operations with null handling
fn compare_with_null<F>(
    engine: &mut ExecutionEngine,
    null_null: bool,
    null_other: bool,
    other_null: bool,
    cmp: F,
    cmp_i64: fn(i64, i64) -> bool,
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
        _ => {
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            match (a_int.to_i64(), b_int.to_i64()) {
                (Some(a), Some(b)) => cmp_i64(a, b),
                _ => cmp(&a_int, &b_int),
            }
        }
    };
    ctx.push(StackItem::from_bool(result))
}

#[inline]
fn lt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(
        engine,
        false,
        true,
        false,
        |a, b| a < b,
        neo_vm_rs::semantics::comparison::less_than_i64,
    )
}

#[inline]
fn le(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(
        engine,
        true,
        true,
        false,
        |a, b| a <= b,
        neo_vm_rs::semantics::comparison::less_or_equal_i64,
    )
}

#[inline]
fn gt(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(
        engine,
        false,
        false,
        true,
        |a, b| a > b,
        neo_vm_rs::semantics::comparison::greater_than_i64,
    )
}

#[inline]
fn ge(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    compare_with_null(
        engine,
        true,
        false,
        true,
        |a, b| a >= b,
        neo_vm_rs::semantics::comparison::greater_or_equal_i64,
    )
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
        (StackItem::Boolean(a_bool), _) => {
            let bi = b.as_int()?;
            if *a_bool {
                bi.is_one()
            } else {
                bi.is_zero()
            }
        }
        (_, StackItem::Boolean(b_bool)) => {
            let ai = a.as_int()?;
            if *b_bool {
                ai.is_one()
            } else {
                ai.is_zero()
            }
        }
        _ => {
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            match (a_int.to_i64(), b_int.to_i64()) {
                (Some(a), Some(b)) => neo_vm_rs::semantics::comparison::num_equal_i64(a, b),
                _ => a_int == b_int,
            }
        }
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
        (StackItem::Boolean(a_bool), _) => {
            let bi = b.as_int()?;
            if *a_bool {
                !bi.is_one()
            } else {
                !bi.is_zero()
            }
        }
        (_, StackItem::Boolean(b_bool)) => {
            let ai = a.as_int()?;
            if *b_bool {
                !ai.is_one()
            } else {
                !ai.is_zero()
            }
        }
        _ => {
            let a_int = a.as_int()?;
            let b_int = b.as_int()?;
            match (a_int.to_i64(), b_int.to_i64()) {
                (Some(a), Some(b)) => neo_vm_rs::semantics::comparison::num_not_equal_i64(a, b),
                _ => a_int != b_int,
            }
        }
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
    ctx.push(StackItem::from_bool(
        neo_vm_rs::semantics::comparison::bool_and(a, b),
    ))
}

fn boolor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_bool()?;
    let a = ctx.pop()?.as_bool()?;
    ctx.push(StackItem::from_bool(
        neo_vm_rs::semantics::comparison::bool_or(a, b),
    ))
}

// ============================================================================
// Advanced Numeric Operations
// ============================================================================

fn modmul(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let modulus = ctx.pop()?.into_int()?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;

    if modulus.is_zero() {
        return Err(VmError::division_by_zero_msg("division"));
    }

    let result = match (a.to_i64(), b.to_i64(), modulus.to_i64()) {
        (Some(a), Some(b), Some(modulus)) => BigInt::from(
            neo_vm_rs::semantics::arithmetic::modmul_i64(a, b, modulus)
                .map_err(VmError::invalid_operation_msg)?,
        ),
        _ => (a * b) % modulus,
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

fn modpow(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let modulus = ctx.pop()?.into_int()?;
    let exponent = ctx.pop()?.into_int()?;
    let base = ctx.pop()?.into_int()?;

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

    let result = match (base.to_i64(), exponent.to_i64(), modulus.to_i64()) {
        (Some(base_i64), Some(exponent_i64), Some(modulus_i64))
            if !base.is_negative() && modulus.is_positive() =>
        {
            BigInt::from(
                neo_vm_rs::semantics::arithmetic::modpow_i64(base_i64, exponent_i64, modulus_i64)
                    .map_err(VmError::invalid_operation_msg)?,
            )
        }
        _ => base.modpow(&exponent, &modulus),
    };
    check_bigint_size(&result)?;
    ctx.push(StackItem::from_int(result))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Integer square root using Newton's method (matches C# BigInteger.Sqrt)
fn integer_sqrt(value: &BigInt) -> BigInt {
    if value.is_zero() || value.is_one() {
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
    if !value.is_positive() {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires positive value",
        ));
    }
    if modulus.is_zero() || modulus.is_one() {
        return Err(VmError::invalid_operation_msg(
            "Modular inverse requires modulus >= 2",
        ));
    }

    let mut r = value.clone();
    let mut old_r = modulus.clone();
    let mut s = BigInt::one();
    let mut old_s = BigInt::zero();

    while !r.is_zero() {
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

    if !((value * &result) % modulus).is_one() {
        return Err(VmError::invalid_operation_msg(
            "No modular inverse exists for the given inputs",
        ));
    }

    Ok(result)
}

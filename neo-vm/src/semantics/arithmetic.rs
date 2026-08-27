//! Shared integer arithmetic semantics for ABI-level NeoVM runtimes.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::{encode_integer, semantics::numeric, StackValue};

/// Return the canonical NeoVM integer result representation.
pub(crate) fn numeric_stack_value(
    value: BigInt,
    overflow_message: &str,
) -> Result<StackValue, String> {
    match numeric::integer_result(value, overflow_message)? {
        numeric::IntegerResult::Small(value) => Ok(StackValue::Integer(value)),
        numeric::IntegerResult::Big(bytes) => Ok(StackValue::BigInteger(bytes)),
    }
}

/// Convert a primitive numeric-compatible value into the canonical BigInt form.
pub(crate) fn numeric_bigint(value: StackValue) -> Result<BigInt, String> {
    match value {
        StackValue::Integer(value) => Ok(BigInt::from(value)),
        StackValue::Boolean(value) => Ok(BigInt::from(if value { 1 } else { 0 })),
        StackValue::ByteString(value) | StackValue::BigInteger(value) => {
            numeric::decode_signed_le_bytes_bigint(&value)
        }
        StackValue::Null => Err("expected integer-compatible value".into()),
        StackValue::Buffer(_)
        | StackValue::Pointer(_)
        | StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_) => Err("expected integer-compatible value".into()),
    }
}

/// Bitwise inversion.
pub fn invert_value(value: StackValue) -> Result<StackValue, String> {
    let value = numeric_bigint(value).map_err(|error| {
        if error == "expected integer-compatible value" {
            "INVERT expects an integer or boolean".to_string()
        } else {
            error
        }
    })?;
    numeric_stack_value(!value, "integer overflow for INVERT")
}

/// Add two integer-compatible values.
pub fn add_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? + numeric_bigint(right)?,
        "integer overflow for ADD",
    )
}

/// Subtract two integer-compatible values.
pub fn sub_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? - numeric_bigint(right)?,
        "integer overflow for SUB",
    )
}

/// Multiply two integer-compatible values.
pub fn mul_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? * numeric_bigint(right)?,
        "integer overflow for MUL",
    )
}

/// Divide two integer-compatible values.
pub fn div_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let right = numeric_bigint(right)?;
    if right.is_zero() {
        return Err("division by zero for DIV".into());
    }
    numeric_stack_value(numeric_bigint(left)? / right, "integer overflow for DIV")
}

/// Compute integer remainder for two integer-compatible values.
pub fn modulo_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let right = numeric_bigint(right)?;
    if right.is_zero() {
        return Err("division by zero for MOD".into());
    }
    numeric_stack_value(numeric_bigint(left)? % right, "integer overflow for MOD")
}

/// Negate an integer-compatible value.
pub fn negate_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(-numeric_bigint(value)?, "integer overflow for NEGATE")
}

/// Return absolute value for an integer-compatible value.
pub fn abs_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric::bigint_abs(numeric_bigint(value)?),
        "integer overflow for ABS",
    )
}

/// Return sign as -1, 0, or 1.
pub fn sign_value(value: StackValue) -> Result<StackValue, String> {
    Ok(StackValue::Integer(numeric::bigint_sign(&numeric_bigint(
        value,
    )?)))
}

/// Increment an integer-compatible value.
pub fn inc_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(value)? + BigInt::from(1),
        "integer overflow for INC",
    )
}

/// Decrement an integer-compatible value.
pub fn dec_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(value)? - BigInt::from(1),
        "integer overflow for DEC",
    )
}

/// Raise `base` to `exponent`.
pub fn pow_values(base: StackValue, exponent: StackValue) -> Result<StackValue, String> {
    let exponent = numeric_bigint(exponent)?;
    if exponent < BigInt::from(0) {
        return Err("negative exponent for POW".into());
    }
    let exponent = exponent
        .to_u32()
        .ok_or_else(|| "exponent too large for POW".to_string())?;
    numeric_stack_value(
        numeric_bigint(base)?.pow(exponent),
        "integer overflow for POW",
    )
}

/// Integer square root.
pub fn sqrt_value(value: StackValue) -> Result<StackValue, String> {
    let value = numeric_bigint(value)?;
    if value < BigInt::from(0) {
        return Err("negative value for SQRT".into());
    }
    numeric_stack_value(value.sqrt(), "integer overflow for SQRT")
}

/// Compute `(left * right) % modulus`.
pub fn modmul_values(
    left: StackValue,
    right: StackValue,
    modulus: StackValue,
) -> Result<StackValue, String> {
    let modulus = numeric_bigint(modulus)?;
    if modulus.is_zero() {
        return Err("division by zero for MODMUL".into());
    }
    numeric_stack_value(
        (numeric_bigint(left)? * numeric_bigint(right)?) % modulus,
        "integer overflow for MODMUL",
    )
}

/// Compute `base^exponent % modulus`, including NeoVM modular inverse support.
pub fn modpow_values(
    base: StackValue,
    exponent: StackValue,
    modulus: StackValue,
) -> Result<StackValue, String> {
    let modulus = numeric_bigint(modulus)?;
    if modulus.is_zero() {
        return Err("division by zero for MODPOW".into());
    }
    numeric_stack_value(
        numeric::mod_pow_bigint(numeric_bigint(base)?, numeric_bigint(exponent)?, modulus)?,
        "integer overflow for MODPOW",
    )
}

/// Shift left. A zero shift preserves the original value.
pub fn shl_value(value: StackValue, shift: i64) -> Result<StackValue, String> {
    if !(0..=256).contains(&shift) {
        return Err("shift count out of range for SHL".into());
    }
    if shift == 0 {
        return Ok(value);
    }
    numeric_stack_value(
        numeric_bigint(value)? << (shift as usize),
        "integer overflow for SHL",
    )
}

/// Arithmetic shift right. A zero shift preserves the original value.
pub fn shr_value(value: StackValue, shift: i64) -> Result<StackValue, String> {
    if !(0..=256).contains(&shift) {
        return Err("shift count out of range for SHR".into());
    }
    if shift == 0 {
        return Ok(value);
    }
    numeric_stack_value(
        numeric_bigint(value)? >> (shift as usize),
        "integer overflow for SHR",
    )
}

/// Bitwise AND.
pub fn bitwise_and_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    bitwise_values(&left, &right, |left, right| left & right)
}

/// Bitwise OR.
pub fn bitwise_or_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    bitwise_values(&left, &right, |left, right| left | right)
}

/// Bitwise XOR.
pub fn bitwise_xor_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    bitwise_values(&left, &right, |left, right| left ^ right)
}

fn bitwise_values<F>(left: &StackValue, right: &StackValue, op: F) -> Result<StackValue, String>
where
    F: Fn(u8, u8) -> u8,
{
    let left_bytes = bitwise_operand_bytes(left)?;
    let right_bytes = bitwise_operand_bytes(right)?;
    bigint_or_integer(numeric::bitwise_signed_bytes(
        &left_bytes,
        &right_bytes,
        op,
    )?)
}

fn bitwise_operand_bytes(value: &StackValue) -> Result<Vec<u8>, String> {
    match value {
        StackValue::Integer(value) => Ok(encode_integer(*value)),
        StackValue::BigInteger(value) | StackValue::ByteString(value) => Ok(value.clone()),
        StackValue::Boolean(value) => Ok(encode_integer(if *value { 1 } else { 0 })),
        StackValue::Null
        | StackValue::Buffer(_)
        | StackValue::Pointer(_)
        | StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_) => {
            Err("bitwise op expects primitive numeric or byte string operands".into())
        }
    }
}

fn bigint_or_integer(bytes: Vec<u8>) -> Result<StackValue, String> {
    let trimmed = numeric::trim_le_bytes(bytes);
    Ok(if trimmed.len() <= 8 {
        StackValue::Integer(numeric::bytes_to_integer(&trimmed))
    } else {
        StackValue::BigInteger(trimmed)
    })
}

/// Return the larger integer-compatible value.
pub fn max_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let left = numeric_bigint(left)?;
    let right = numeric_bigint(right)?;
    numeric_stack_value(
        if left > right { left } else { right },
        "integer overflow for MAX",
    )
}

/// Return the smaller integer-compatible value.
pub fn min_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let left = numeric_bigint(left)?;
    let right = numeric_bigint(right)?;
    numeric_stack_value(
        if left < right { left } else { right },
        "integer overflow for MIN",
    )
}

/// Return whether `lower <= value < upper`.
pub fn within_values(
    value: StackValue,
    lower: StackValue,
    upper: StackValue,
) -> Result<bool, String> {
    let value = numeric_bigint(value)?;
    let lower = numeric_bigint(lower)?;
    let upper = numeric_bigint(upper)?;
    Ok(value >= lower && value < upper)
}

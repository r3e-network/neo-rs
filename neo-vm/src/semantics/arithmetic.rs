//! Shared integer arithmetic semantics for ABI-level NeoVM runtimes.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use crate::{StackValue, encode_integer, semantics::numeric};

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
#[must_use]
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
#[must_use]
pub fn add_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? + numeric_bigint(right)?,
        "integer overflow for ADD",
    )
}

/// Subtract two integer-compatible values.
#[must_use]
pub fn sub_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? - numeric_bigint(right)?,
        "integer overflow for SUB",
    )
}

/// Multiply two integer-compatible values.
#[must_use]
pub fn mul_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(left)? * numeric_bigint(right)?,
        "integer overflow for MUL",
    )
}

/// Divide two integer-compatible values.
#[must_use]
pub fn div_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let right = numeric_bigint(right)?;
    if right.is_zero() {
        return Err("division by zero for DIV".into());
    }
    numeric_stack_value(numeric_bigint(left)? / right, "integer overflow for DIV")
}

/// Compute integer remainder for two integer-compatible values.
#[must_use]
pub fn modulo_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let right = numeric_bigint(right)?;
    if right.is_zero() {
        return Err("division by zero for MOD".into());
    }
    numeric_stack_value(numeric_bigint(left)? % right, "integer overflow for MOD")
}

/// Negate an integer-compatible value.
#[must_use]
pub fn negate_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(-numeric_bigint(value)?, "integer overflow for NEGATE")
}

/// Return absolute value for an integer-compatible value.
#[must_use]
pub fn abs_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric::bigint_abs(numeric_bigint(value)?),
        "integer overflow for ABS",
    )
}

/// Return sign as -1, 0, or 1.
#[must_use]
pub fn sign_value(value: StackValue) -> Result<StackValue, String> {
    Ok(StackValue::Integer(numeric::bigint_sign(&numeric_bigint(
        value,
    )?)))
}

/// Increment an integer-compatible value.
#[must_use]
pub fn inc_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(value)? + BigInt::from(1),
        "integer overflow for INC",
    )
}

/// Decrement an integer-compatible value.
#[must_use]
pub fn dec_value(value: StackValue) -> Result<StackValue, String> {
    numeric_stack_value(
        numeric_bigint(value)? - BigInt::from(1),
        "integer overflow for DEC",
    )
}

/// Raise `base` to `exponent`.
///
/// NeoVM borrows the shift limit for `POW`: the exponent must fit in an `int`
/// and must be in `0..=MaxShift` (256). This mirrors C#
/// `ExecutionEngineLimits.AssertShift` before `BigInteger.Pow`. The result is
/// still checked against the 32-byte integer limit, exactly like the C#
/// `Integer` constructor. Do not add a pre-flight size estimate here: it
/// changes the accepted language (e.g. `1^1000` and `0^1000` must FAULT on the
/// exponent check, not silently produce a result).
#[must_use]
pub fn pow_values(base: StackValue, exponent: StackValue) -> Result<StackValue, String> {
    let base_big = numeric_bigint(base)?;
    let exp_big = numeric_bigint(exponent)?;

    let exponent = exp_big
        .to_i32()
        .ok_or_else(|| "exponent too large for POW".to_string())?;
    if !(0..=256).contains(&exponent) {
        return Err("shift count out of range for POW".to_string());
    }

    numeric_stack_value(base_big.pow(exponent as u32), "integer overflow for POW")
}

/// Integer square root.
#[must_use]
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

/// Shift left. The result is always an `Integer` (Gorgon hardfork semantics:
/// a zero shift converts the operand to `Integer` instead of preserving its
/// original stack item type).
#[must_use]
pub fn shl_value(value: StackValue, shift: i64) -> Result<StackValue, String> {
    if !(0..=256).contains(&shift) {
        return Err("shift count out of range for SHL".into());
    }
    numeric_stack_value(
        numeric_bigint(value)? << (shift as usize),
        "integer overflow for SHL",
    )
}

/// Arithmetic shift right. The result is always an `Integer` (Gorgon hardfork
/// semantics: a zero shift converts the operand to `Integer` instead of
/// preserving its original stack item type).
#[must_use]
pub fn shr_value(value: StackValue, shift: i64) -> Result<StackValue, String> {
    if !(0..=256).contains(&shift) {
        return Err("shift count out of range for SHR".into());
    }
    numeric_stack_value(
        numeric_bigint(value)? >> (shift as usize),
        "integer overflow for SHR",
    )
}

/// Pre-Gorgon shift left: a zero shift preserves the operand's original type.
#[must_use]
pub fn shl_value_pre_gorgon(value: StackValue, shift: i64) -> Result<StackValue, String> {
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

/// Pre-Gorgon arithmetic shift right: a zero shift preserves the operand's
/// original type.
#[must_use]
pub fn shr_value_pre_gorgon(value: StackValue, shift: i64) -> Result<StackValue, String> {
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
#[must_use]
pub fn bitwise_and_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    bitwise_values(&left, &right, |left, right| left & right)
}

/// Bitwise OR.
#[must_use]
pub fn bitwise_or_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    bitwise_values(&left, &right, |left, right| left | right)
}

/// Bitwise XOR.
#[must_use]
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
#[must_use]
pub fn max_values(left: StackValue, right: StackValue) -> Result<StackValue, String> {
    let left = numeric_bigint(left)?;
    let right = numeric_bigint(right)?;
    numeric_stack_value(
        if left > right { left } else { right },
        "integer overflow for MAX",
    )
}

/// Return the smaller integer-compatible value.
#[must_use]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shl_zero_shift_yields_integer_gorgon() {
        // R06: post-Gorgon, a zero shift converts the operand to Integer
        // instead of preserving its original stack item type.
        let result = shl_value(StackValue::ByteString(vec![0x01]), 0).unwrap();
        assert!(matches!(result, StackValue::Integer(1)));
    }

    #[test]
    fn shr_zero_shift_yields_integer_gorgon() {
        let result = shr_value(StackValue::ByteString(vec![0x04]), 0).unwrap();
        assert!(matches!(result, StackValue::Integer(4)));
    }

    #[test]
    fn shl_nonzero_shift_yields_integer() {
        let result = shl_value(StackValue::Integer(1), 3).unwrap();
        assert_eq!(result, StackValue::Integer(8));
    }

    #[test]
    fn shl_pre_gorgon_zero_shift_preserves_type() {
        let result = shl_value_pre_gorgon(StackValue::ByteString(vec![0x01]), 0).unwrap();
        assert_eq!(result, StackValue::ByteString(vec![0x01]));
    }

    #[test]
    fn shift_count_out_of_range_faults() {
        assert!(shl_value(StackValue::Integer(1), 257).is_err());
        assert!(shl_value(StackValue::Integer(1), -1).is_err());
    }

    #[test]
    fn pow_uses_the_shift_limit_not_a_bit_estimate() {
        // Identity bases must still be rejected when the exponent exceeds
        // MaxShift: C# calls AssertShift before BigInteger.Pow, so POW(1, 1000)
        // and POW(0, 300) FAULT. An estimated-bits shortcut would accept them.
        assert!(pow_values(StackValue::Integer(1), StackValue::Integer(1000)).is_err());
        assert!(pow_values(StackValue::Integer(0), StackValue::Integer(300)).is_err());
        assert!(pow_values(StackValue::Integer(2), StackValue::Integer(257)).is_err());
        assert!(pow_values(StackValue::Integer(2), StackValue::Integer(-1)).is_err());
    }

    #[test]
    fn pow_accepts_limit_and_enforces_32_byte_integer() {
        assert_eq!(
            pow_values(StackValue::Integer(1), StackValue::Integer(256)).unwrap(),
            StackValue::Integer(1)
        );
        assert_eq!(
            pow_values(StackValue::Integer(-1), StackValue::Integer(255)).unwrap(),
            StackValue::Integer(-1)
        );

        // 2^254 is the largest power of two representable as a 32-byte signed
        // BigInteger; 2^255 needs a leading zero byte (33 bytes) in C# and must
        // therefore FAULT.
        assert!(pow_values(StackValue::Integer(2), StackValue::Integer(254)).is_ok());
        assert!(pow_values(StackValue::Integer(2), StackValue::Integer(255)).is_err());
    }
}

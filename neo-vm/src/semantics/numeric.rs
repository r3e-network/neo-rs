//! Shared little-endian two's-complement integer helpers.

use alloc::{string::String, string::ToString, vec, vec::Vec};

use num_bigint::BigInt;
use num_traits::Zero;

pub(crate) const MAX_INTEGER_SIZE: usize = 32;

pub(crate) enum IntegerResult {
    Small(i64),
    Big(Vec<u8>),
}

pub(crate) fn decode_signed_le_bytes_i64(bytes: &[u8]) -> Result<i64, String> {
    if bytes.is_empty() {
        return Ok(0);
    }
    if bytes.len() > MAX_INTEGER_SIZE {
        return Err("integer size exceeds maximum".to_string());
    }

    let sign_extend = if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };

    if bytes.len() > 8 {
        if bytes.iter().all(|byte| *byte == 0) {
            return Ok(0);
        }

        if bytes[8..].iter().all(|byte| *byte == sign_extend)
            && ((bytes[7] & 0x80) == (sign_extend & 0x80))
        {
            let mut buffer = [sign_extend; 8];
            buffer.copy_from_slice(&bytes[..8]);
            return Ok(i64::from_le_bytes(buffer));
        }

        return Err("integer exceeds i64 range".to_string());
    }

    let mut buffer = [sign_extend; 8];
    buffer[..bytes.len()].copy_from_slice(bytes);
    Ok(i64::from_le_bytes(buffer))
}

pub(crate) fn decode_signed_le_bytes_i128(bytes: &[u8]) -> Result<i128, String> {
    if bytes.is_empty() {
        return Ok(0);
    }
    if bytes.len() > 16 {
        return Err("integer exceeds i128 range".to_string());
    }

    let sign_extend = if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };

    let mut buffer = [sign_extend; 16];
    buffer[..bytes.len()].copy_from_slice(bytes);
    Ok(i128::from_le_bytes(buffer))
}

pub(crate) fn decode_signed_le_bytes_bigint(bytes: &[u8]) -> Result<BigInt, String> {
    if bytes.len() > MAX_INTEGER_SIZE {
        return Err("integer size exceeds maximum".to_string());
    }
    Ok(BigInt::from_signed_bytes_le(bytes))
}

pub(crate) fn integer_result(
    value: BigInt,
    overflow_message: &str,
) -> Result<IntegerResult, String> {
    let bytes = trim_le_bytes(value.to_signed_bytes_le());
    if bytes.len() > MAX_INTEGER_SIZE {
        return Err(overflow_message.to_string());
    }

    if bytes.len() <= 8 {
        Ok(IntegerResult::Small(bytes_to_integer(&bytes)))
    } else {
        Ok(IntegerResult::Big(bytes))
    }
}

pub(crate) fn trim_le_bytes(mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_empty() {
        return bytes;
    }

    let sign_extend = if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };
    while bytes.len() > 1 && bytes.last() == Some(&sign_extend) {
        let next = bytes[bytes.len() - 2];
        if (next & 0x80 != 0) == (sign_extend == 0xff) {
            bytes.pop();
        } else {
            break;
        }
    }
    bytes
}

pub(crate) fn trim_le_bytes_slice(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return Vec::new();
    }

    let sign_extend = if bytes.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };
    let mut end = bytes.len();
    while end > 1 && bytes[end - 1] == sign_extend {
        let next = bytes[end - 2];
        if (next & 0x80 != 0) == (sign_extend == 0xff) {
            end -= 1;
        } else {
            break;
        }
    }
    bytes[..end].to_vec()
}

pub(crate) fn bytes_to_integer(bytes: &[u8]) -> i64 {
    if bytes.is_empty() {
        return 0;
    }

    let negative = bytes.last().is_some_and(|byte| byte & 0x80 != 0);
    let mut buffer = [0u8; 8];
    let copy_len = bytes.len().min(8);
    buffer[..copy_len].copy_from_slice(&bytes[..copy_len]);

    if negative && copy_len < 8 {
        for byte in &mut buffer[copy_len..] {
            *byte = 0xff;
        }
    }

    i64::from_le_bytes(buffer)
}

pub(crate) fn bigint_sign(value: &BigInt) -> i64 {
    if value.is_zero() {
        0
    } else if value < &BigInt::zero() {
        -1
    } else {
        1
    }
}

pub(crate) fn bigint_abs(value: BigInt) -> BigInt {
    if value < BigInt::zero() {
        -value
    } else {
        value
    }
}

pub(crate) fn mod_pow_bigint(
    base: BigInt,
    exponent: BigInt,
    modulus: BigInt,
) -> Result<BigInt, String> {
    if modulus.is_zero() {
        return Err("division by zero for MODPOW".to_string());
    }

    if exponent == BigInt::from(-1) {
        if base <= BigInt::zero() {
            return Err("value has no modular inverse".to_string());
        }
        if modulus <= BigInt::from(1) {
            return Err("invalid modulus for modular inverse".to_string());
        }
        return base
            .modinv(&modulus)
            .ok_or_else(|| "value is not invertible for MODPOW".to_string());
    }

    if exponent < BigInt::zero() {
        return Err("negative exponent for MODPOW".to_string());
    }

    let mut result = BigInt::from(1) % &modulus;
    let mut power = base % &modulus;
    let mut exponent = exponent;
    while exponent > BigInt::zero() {
        if (&exponent % 2u8) != BigInt::zero() {
            result = (result * &power) % &modulus;
        }
        exponent >>= 1usize;
        if exponent > BigInt::zero() {
            power = (&power * &power) % &modulus;
        }
    }
    Ok(result)
}

pub(crate) fn bitwise_signed_bytes<F>(left: &[u8], right: &[u8], op: F) -> Result<Vec<u8>, String>
where
    F: Fn(u8, u8) -> u8,
{
    let len = left.len().max(right.len());
    if len == 0 {
        return Ok(vec![op(0, 0)]);
    }
    if len > MAX_INTEGER_SIZE {
        return Err("bitwise operand exceeds supported size".to_string());
    }

    let left_fill = if left.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };
    let right_fill = if right.last().is_some_and(|byte| byte & 0x80 != 0) {
        0xff
    } else {
        0x00
    };

    let mut result = Vec::with_capacity(len);
    for index in 0..len {
        let left_byte = left.get(index).copied().unwrap_or(left_fill);
        let right_byte = right.get(index).copied().unwrap_or(right_fill);
        result.push(op(left_byte, right_byte));
    }
    Ok(result)
}

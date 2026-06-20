//! Shared argument-parsing helpers for native contracts.
//!
//! Mirrors the C# `StackItem` → `UInt160` / `UInt256` / integer coercions that
//! every native-contract method opens with. Keeps the per-method body focused
//! on its business logic instead of repeating the same 3-line decode/error
//! dance.

use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

// ===== `&[StackItem]`-based helpers (consume the engine's already-decoded
// stack items; used by native contracts whose `invoke` receives `Vec<StackItem>`). =====

/// Returns the i-th argument from `args`, raising a `CoreError::invalid_operation`
/// if absent.
pub(crate) fn arg<'a>(
    args: &'a [StackItem],
    index: usize,
    method: &str,
) -> CoreResult<&'a StackItem> {
    args.get(index).ok_or_else(|| {
        CoreError::invalid_operation(format!(
            "{method}: expected at least {} argument(s), got {}",
            index + 1,
            args.len()
        ))
    })
}

/// Decodes the i-th argument as a `UInt160` (`Hash160`).
pub(crate) fn hash160_arg(args: &[StackItem], index: usize, method: &str) -> CoreResult<UInt160> {
    let bytes = arg(args, index, method)?.as_bytes().map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a ByteString: {e}"))
    })?;
    UInt160::from_bytes(&bytes).map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash160: {e}"))
    })
}

/// Decodes the i-th argument as a `UInt256` (`Hash256`).
pub(crate) fn hash256_arg(args: &[StackItem], index: usize, method: &str) -> CoreResult<UInt256> {
    let bytes = arg(args, index, method)?.as_bytes().map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a ByteString: {e}"))
    })?;
    UInt256::from_bytes(&bytes).map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash256: {e}"))
    })
}

/// Decodes the i-th argument as a `i64` integer (used by PolicyContract
/// setter methods).
pub(crate) fn setter_int_arg(args: &[StackItem], method: &str) -> CoreResult<i64> {
    let value = arg(args, 0, method)?
        .as_int()
        .map_err(|e| CoreError::invalid_operation(format!("{method}: expected integer: {e}")))?;
    value
        .to_i64()
        .ok_or_else(|| CoreError::invalid_operation(format!("{method}: integer out of i64 range")))
}

// ===== `&[Vec<u8>]`-based helpers (consume the raw args; used by native
// contracts whose `invoke` is invoked with the raw `Vec<u8>` form from the
// call-site disassembly). =====

/// Returns the i-th raw `Vec<u8>` argument, raising `CoreError::invalid_operation`
/// if absent.
pub(crate) fn raw_arg<'a>(args: &'a [Vec<u8>], index: usize, method: &str) -> CoreResult<&'a [u8]> {
    args.get(index).map(|v| v.as_slice()).ok_or_else(|| {
        CoreError::invalid_operation(format!(
            "{method}: expected at least {} argument(s), got {}",
            index + 1,
            args.len()
        ))
    })
}

/// Decodes the i-th raw argument as a Neo VM integer.
pub(crate) fn raw_integer_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<BigInt> {
    Ok(raw_integer_bytes(raw_arg(args, index, method)?))
}

/// Decodes the i-th raw argument as a Neo VM integer, using a domain-specific
/// missing-argument description such as `"an amount"`.
pub(crate) fn raw_required_integer_arg(
    args: &[Vec<u8>],
    index: usize,
    method: &str,
    missing: &str,
) -> CoreResult<BigInt> {
    let bytes = raw_arg(args, index, method)
        .map_err(|_| CoreError::invalid_operation(format!("{method} requires {missing}")))?;
    Ok(raw_integer_bytes(bytes))
}

/// Decodes the i-th raw argument as an `i64`.
pub(crate) fn raw_i64_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<i64> {
    raw_integer_bytes_to_i64(
        raw_arg(args, index, method)?,
        &format!("{method}: arg {index}"),
    )
}

/// Decodes the i-th raw argument as a `u32`.
pub(crate) fn raw_u32_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<u32> {
    raw_integer_bytes_to_u32(
        raw_arg(args, index, method)?,
        &format!("{method}: arg {index}"),
    )
}

/// Decodes the i-th raw argument as an `i32`.
pub(crate) fn raw_i32_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<i32> {
    raw_integer_bytes_to_i32(
        raw_arg(args, index, method)?,
        &format!("{method}: arg {index}"),
    )
}

/// Decodes the i-th raw argument as a `u8`.
pub(crate) fn raw_u8_arg(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<u8> {
    raw_integer_bytes_to_u8(
        raw_arg(args, index, method)?,
        &format!("{method}: arg {index}"),
    )
}

/// Decodes the i-th raw argument as a UTF-8 string.
pub(crate) fn raw_string_arg(
    args: &[Vec<u8>],
    index: usize,
    method: &str,
    arg_name: &str,
) -> CoreResult<String> {
    let bytes = raw_arg(args, index, method)
        .map_err(|_| CoreError::invalid_operation(format!("{method} requires a {arg_name}")))?;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| CoreError::invalid_operation(format!("{method}: bad {arg_name}: {e}")))
}

/// Decodes the i-th raw argument as a `UInt160`.
pub(crate) fn raw_hash160(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<UInt160> {
    let bytes = raw_arg(args, index, method)?;
    UInt160::from_bytes(bytes).map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash160: {e}"))
    })
}

/// Decodes the i-th raw argument as a `UInt256`.
pub(crate) fn raw_hash256(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<UInt256> {
    let bytes = raw_arg(args, index, method)?;
    UInt256::from_bytes(bytes).map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash256: {e}"))
    })
}

/// Decodes the leading raw argument as a `UInt160` (positional, index 0).
pub(crate) fn raw_account(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
    raw_hash160(args, 0, method)
}

// ===== Byte-slice helpers (operate on already-extracted `&[u8]` from a
// struct field; do not index into `args`). =====

/// Decodes already-extracted raw VM integer bytes.
pub(crate) fn raw_integer_bytes(bytes: &[u8]) -> BigInt {
    BigInt::from_signed_bytes_le(bytes)
}

/// Decodes already-extracted raw VM integer bytes as an `i64`.
pub(crate) fn raw_integer_bytes_to_i64(bytes: &[u8], label: &str) -> CoreResult<i64> {
    raw_integer_bytes(bytes)
        .to_i64()
        .ok_or_else(|| CoreError::invalid_operation(format!("{label} out of i64 range")))
}

/// Decodes already-extracted raw VM integer bytes as a `u32`.
pub(crate) fn raw_integer_bytes_to_u32(bytes: &[u8], label: &str) -> CoreResult<u32> {
    raw_integer_bytes(bytes)
        .to_u32()
        .ok_or_else(|| CoreError::invalid_operation(format!("{label} out of u32 range")))
}

/// Decodes already-extracted raw VM integer bytes as an `i32`.
pub(crate) fn raw_integer_bytes_to_i32(bytes: &[u8], label: &str) -> CoreResult<i32> {
    raw_integer_bytes(bytes)
        .to_i32()
        .ok_or_else(|| CoreError::invalid_operation(format!("{label} out of i32 range")))
}

/// Decodes already-extracted raw VM integer bytes as a `u8`.
pub(crate) fn raw_integer_bytes_to_u8(bytes: &[u8], label: &str) -> CoreResult<u8> {
    raw_integer_bytes(bytes)
        .to_u8()
        .ok_or_else(|| CoreError::invalid_operation(format!("{label} out of u8 range")))
}

/// Decodes a `&[u8]` as a `UInt160`. Returns
/// `CoreError::invalid_data(format!("{label}: {e}"))` on parse failure.
pub(crate) fn bytes_to_hash160(bytes: &[u8], label: &str) -> CoreResult<UInt160> {
    UInt160::from_bytes(bytes).map_err(|e| CoreError::invalid_data(format!("{label}: {e}")))
}

/// Decodes a `&[u8]` as a `UInt256`. Returns
/// `CoreError::invalid_data(format!("{label}: {e}"))` on parse failure.
pub(crate) fn bytes_to_hash256(bytes: &[u8], label: &str) -> CoreResult<UInt256> {
    UInt256::from_bytes(bytes).map_err(|e| CoreError::invalid_data(format!("{label}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_integer_helpers_decode_vm_signed_little_endian_bytes() {
        let args = vec![
            BigInt::from(-1).to_signed_bytes_le(),
            BigInt::from(0x1234_u32).to_signed_bytes_le(),
            BigInt::from(0x7f_i32).to_signed_bytes_le(),
        ];

        assert_eq!(raw_i64_arg(&args, 0, "test").unwrap(), -1);
        assert_eq!(raw_u32_arg(&args, 1, "test").unwrap(), 0x1234);
        assert_eq!(raw_i32_arg(&args, 1, "test").unwrap(), 0x1234);
        assert_eq!(raw_u8_arg(&args, 2, "test").unwrap(), 0x7f);
    }

    #[test]
    fn raw_integer_helpers_reject_missing_or_out_of_range_args() {
        let too_large_for_u8 = vec![BigInt::from(256_u16).to_signed_bytes_le()];
        assert!(raw_u8_arg(&too_large_for_u8, 0, "test").is_err());
        assert!(raw_i64_arg(&[], 0, "test").is_err());
    }

    #[test]
    fn raw_integer_byte_helpers_decode_vm_signed_little_endian_bytes() {
        let positive = BigInt::from(0x1234_u32).to_signed_bytes_le();
        let negative = BigInt::from(-1).to_signed_bytes_le();

        assert_eq!(raw_integer_bytes(&positive), BigInt::from(0x1234_u32));
        assert_eq!(
            raw_integer_bytes_to_u32(&positive, "value").unwrap(),
            0x1234
        );
        assert_eq!(
            raw_integer_bytes_to_i32(&positive, "value").unwrap(),
            0x1234
        );
        assert_eq!(raw_integer_bytes_to_i64(&negative, "value").unwrap(), -1);
        assert_eq!(raw_integer_bytes_to_u32(&[], "empty").unwrap(), 0);
        assert!(raw_integer_bytes_to_u8(&positive, "value").is_err());
        assert!(raw_integer_bytes_to_u32(&negative, "value").is_err());
    }

    #[test]
    fn raw_required_integer_arg_preserves_domain_missing_context() {
        let args = vec![BigInt::from(42).to_signed_bytes_le()];
        assert_eq!(
            raw_required_integer_arg(&args, 0, "Token::transfer", "an amount").unwrap(),
            BigInt::from(42)
        );

        let missing = raw_required_integer_arg(&[], 0, "Token::transfer", "an amount")
            .expect_err("missing named integer should fault");
        assert!(missing.to_string().contains("requires an amount"));
    }

    #[test]
    fn raw_string_arg_decodes_utf8_with_named_errors() {
        let args = vec![b"balanceOf".to_vec()];
        assert_eq!(
            raw_string_arg(&args, 0, "Contract::method", "method name").unwrap(),
            "balanceOf"
        );

        let missing = raw_string_arg(&[], 0, "Contract::method", "method name")
            .expect_err("missing string arg should fault");
        assert!(missing.to_string().contains("requires a method name"));

        let invalid = raw_string_arg(&[vec![0xff]], 0, "Contract::method", "method name")
            .expect_err("invalid UTF-8 should fault");
        assert!(invalid.to_string().contains("bad method name"));
    }
}

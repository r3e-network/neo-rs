//! Shared argument-parsing helpers for native contracts.
//!
//! Mirrors the C# `StackItem` → `UInt160` / `UInt256` / integer coercions that
//! every native-contract method opens with. Keeps the per-method body focused
//! on its business logic instead of repeating the same 3-line decode/error
//! dance.

use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

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
#[path = "../tests/support/args.rs"]
mod tests;

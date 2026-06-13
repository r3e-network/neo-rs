//! Shared argument-parsing helpers for native contracts.
//!
//! Mirrors the C# `StackItem` → `UInt160` / `UInt256` / integer coercions that
//! every native-contract method opens with. Keeps the per-method body focused
//! on its business logic instead of repeating the same 3-line decode/error
//! dance.

use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_vm::StackItem;
use num_traits::ToPrimitive;

// ===== `&[StackItem]`-based helpers (consume the engine's already-decoded
// stack items; used by native contracts whose `invoke` receives `Vec<StackItem>`). =====

/// Returns the i-th argument from `args`, raising a `CoreError::invalid_operation`
/// if absent.
pub(crate) fn arg<'a>(args: &'a [StackItem], index: usize, method: &str) -> CoreResult<&'a StackItem> {
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
    let bytes = arg(args, index, method)?
        .as_bytes()
        .map_err(|e| CoreError::invalid_operation(format!("{method}: arg {index} is not a ByteString: {e}")))?;
    UInt160::from_bytes(&bytes).map_err(|e| {
        CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash160: {e}"))
    })
}

/// Decodes the i-th argument as a `UInt256` (`Hash256`).
pub(crate) fn hash256_arg(args: &[StackItem], index: usize, method: &str) -> CoreResult<UInt256> {
    let bytes = arg(args, index, method)?
        .as_bytes()
        .map_err(|e| CoreError::invalid_operation(format!("{method}: arg {index} is not a ByteString: {e}")))?;
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
    value.to_i64().ok_or_else(|| {
        CoreError::invalid_operation(format!("{method}: integer out of i64 range"))
    })
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

/// Decodes the i-th raw argument as a `UInt160`.
pub(crate) fn raw_hash160(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<UInt160> {
    let bytes = raw_arg(args, index, method)?;
    UInt160::from_bytes(bytes)
        .map_err(|e| CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash160: {e}")))
}

/// Decodes the i-th raw argument as a `UInt256`.
pub(crate) fn raw_hash256(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<UInt256> {
    let bytes = raw_arg(args, index, method)?;
    UInt256::from_bytes(bytes)
        .map_err(|e| CoreError::invalid_operation(format!("{method}: arg {index} is not a valid Hash256: {e}")))
}

/// Decodes the leading raw argument as a `UInt160` (positional, index 0).
pub(crate) fn raw_account(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
    raw_hash160(args, 0, method)
}

// ===== Byte-slice helpers (operate on already-extracted `&[u8]` from a
// struct field; do not index into `args`). =====

/// Decodes a `&[u8]` as a `UInt160`. Returns
/// `CoreError::invalid_data(format!("{label}: {e}"))` on parse failure.
pub(crate) fn bytes_to_hash160(bytes: &[u8], label: &str) -> CoreResult<UInt160> {
    UInt160::from_bytes(bytes)
        .map_err(|e| CoreError::invalid_data(format!("{label}: {e}")))
}

/// Decodes a `&[u8]` as a `UInt256`. Returns
/// `CoreError::invalid_data(format!("{label}: {e}"))` on parse failure.
pub(crate) fn bytes_to_hash256(bytes: &[u8], label: &str) -> CoreResult<UInt256> {
    UInt256::from_bytes(bytes)
        .map_err(|e| CoreError::invalid_data(format!("{label}: {e}")))
}

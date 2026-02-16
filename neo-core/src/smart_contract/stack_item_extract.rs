//! Shared extraction helpers for common `StackItem` decoding patterns.

use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Returns the stack item as bytes when supported by its representation.
pub fn extract_bytes(item: &StackItem) -> Option<Vec<u8>> {
    item.as_bytes().ok()
}

/// Returns a UTF-8 string extracted from a byte-backed stack item.
pub fn extract_string(item: &StackItem) -> Option<String> {
    extract_bytes(item).and_then(|bytes| String::from_utf8(bytes).ok())
}

/// Returns the stack item value as `u8` when it is an integer in range.
pub fn extract_u8(item: &StackItem) -> Option<u8> {
    item.as_int().ok().and_then(|value| value.to_u8())
}

/// Returns the stack item value as `u32` when it is an integer in range.
pub fn extract_u32(item: &StackItem) -> Option<u32> {
    item.as_int().ok().and_then(|value| value.to_u32())
}

/// Returns the stack item value as `i32` when it is an integer in range.
pub fn extract_i32(item: &StackItem) -> Option<i32> {
    item.as_int().ok().and_then(|value| value.to_i32())
}

/// Returns the stack item value as `i64` when it is an integer in range.
pub fn extract_i64(item: &StackItem) -> Option<i64> {
    item.as_int().ok().and_then(|value| value.to_i64())
}

/// Returns the stack item value as `bool` when it is a VM boolean.
pub fn extract_bool(item: &StackItem) -> Option<bool> {
    item.as_bool().ok()
}

/// Returns the stack item value as a `BigInt` when it is an integer.
pub fn extract_int(item: &StackItem) -> Option<BigInt> {
    item.as_int().ok()
}

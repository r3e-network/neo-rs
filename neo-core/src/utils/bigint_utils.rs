//! BigInt conversion utilities to reduce duplication across modules.
//!
//! This module provides reusable helper functions for common BigInt operations
//! that appear frequently throughout the codebase.

use crate::error::{CoreError, CoreResult};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Converts bytes to BigInt using signed little-endian format.
///
/// This is a common pattern in Neo smart contract operations.
#[inline]
pub fn bytes_to_bigint_le(bytes: &[u8]) -> BigInt {
    BigInt::from_signed_bytes_le(bytes)
}

/// Converts BigInt to bytes using signed little-endian format.
///
/// This is a common pattern in Neo smart contract operations.
#[inline]
pub fn bigint_to_bytes_le(value: &BigInt) -> Vec<u8> {
    value.to_signed_bytes_le()
}

/// Converts BigInt to i32 with error handling.
///
/// Returns an error if the conversion fails due to overflow.
pub fn bigint_to_i32(value: &BigInt, context: &str) -> CoreResult<i32> {
    value.to_i32().ok_or_else(|| {
        CoreError::native_contract(format!(
            "{}: Integer overflow converting BigInt to i32",
            context
        ))
    })
}

/// Converts BigInt to i64 with error handling.
///
/// Returns an error if the conversion fails due to overflow.
pub fn bigint_to_i64(value: &BigInt, context: &str) -> CoreResult<i64> {
    value.to_i64().ok_or_else(|| {
        CoreError::native_contract(format!(
            "{}: Integer overflow converting BigInt to i64",
            context
        ))
    })
}

/// Converts BigInt to u32 with error handling.
///
/// Returns an error if the conversion fails due to overflow or negative value.
pub fn bigint_to_u32(value: &BigInt, context: &str) -> CoreResult<u32> {
    value.to_u32().ok_or_else(|| {
        CoreError::native_contract(format!("{}: Cannot convert BigInt to u32", context))
    })
}

/// Converts BigInt to u64 with error handling.
///
/// Returns an error if the conversion fails due to overflow or negative value.
pub fn bigint_to_u64(value: &BigInt, context: &str) -> CoreResult<u64> {
    value.to_u64().ok_or_else(|| {
        CoreError::native_contract(format!("{}: Cannot convert BigInt to u64", context))
    })
}

/// Converts BigInt to usize with error handling.
///
/// Returns an error if the conversion fails due to overflow or negative value.
pub fn bigint_to_usize(value: &BigInt, context: &str) -> CoreResult<usize> {
    value.to_usize().ok_or_else(|| {
        CoreError::native_contract(format!("{}: Cannot convert BigInt to usize", context))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_bigint_le() {
        let bytes = vec![1, 0, 0, 0];
        let bigint = bytes_to_bigint_le(&bytes);
        assert_eq!(bigint, BigInt::from(1));
    }

    #[test]
    fn test_bigint_to_bytes_le() {
        let bigint = BigInt::from(256);
        let bytes = bigint_to_bytes_le(&bigint);
        assert_eq!(bytes, vec![0, 1]);
    }

    #[test]
    fn test_bigint_to_i32_success() {
        let bigint = BigInt::from(42);
        let result = bigint_to_i32(&bigint, "test").unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn test_bigint_to_i32_overflow() {
        let bigint = BigInt::from(i64::MAX);
        let result = bigint_to_i32(&bigint, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_bigint_to_u32_success() {
        let bigint = BigInt::from(100u32);
        let result = bigint_to_u32(&bigint, "test").unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_bigint_to_u32_negative() {
        let bigint = BigInt::from(-1);
        let result = bigint_to_u32(&bigint, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_bigint_to_usize_success() {
        let bigint = BigInt::from(50);
        let result = bigint_to_usize(&bigint, "test").unwrap();
        assert_eq!(result, 50);
    }
}

//! Common error handling utilities for encoding operations.
//!
//! This module provides reusable error mapping functions to reduce duplication
//! in encoding/decoding operations.

use crate::error::{CryptoError, CryptoResult};

/// Maps a generic error to a CryptoError with an encoding error message.
///
/// This helper reduces duplication of error mapping patterns in encoding functions.
#[inline]
pub fn map_encoding_error<E: std::fmt::Display>(error: E, context: &str) -> CryptoError {
    CryptoError::encoding_error(format!("{}: {}", context, error))
}

/// Wraps a Result with consistent encoding error handling.
///
/// This is useful for chaining encoding operations with `.or_else()`.
pub fn encoding_result<T, E: std::fmt::Display>(
    result: Result<T, E>,
    context: &str,
) -> CryptoResult<T> {
    result.map_err(|e| map_encoding_error(e, context))
}

/// Common error mapper for Base58 decode operations.
#[inline]
pub fn map_base58_error<E: std::fmt::Display>(error: E) -> CryptoError {
    map_encoding_error(error, "Base58 decode error")
}

/// Common error mapper for Base64 decode operations.
#[inline]
pub fn map_base64_error<E: std::fmt::Display>(error: E) -> CryptoError {
    map_encoding_error(error, "Base64 decode error")
}

/// Common error mapper for Hex decode operations.
#[inline]
pub fn map_hex_error<E: std::fmt::Display>(error: E) -> CryptoError {
    map_encoding_error(error, "Hex decode error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_encoding_error() {
        let err = map_encoding_error("invalid input", "Test operation");
        assert!(err.to_string().contains("Test operation"));
        assert!(err.to_string().contains("invalid input"));
    }

    #[test]
    fn test_encoding_result_ok() {
        let result: Result<i32, &str> = Ok(42);
        let mapped = encoding_result(result, "Test");
        assert_eq!(mapped.unwrap(), 42);
    }

    #[test]
    fn test_encoding_result_err() {
        let result: Result<i32, &str> = Err("failed");
        let mapped = encoding_result(result, "Test");
        assert!(mapped.is_err());
        assert!(mapped.unwrap_err().to_string().contains("Test"));
    }

    #[test]
    fn test_map_base58_error() {
        let err = map_base58_error("invalid character");
        assert!(err.to_string().contains("Base58"));
    }

    #[test]
    fn test_map_base64_error() {
        let err = map_base64_error("invalid padding");
        assert!(err.to_string().contains("Base64"));
    }

    #[test]
    fn test_map_hex_error() {
        let err = map_hex_error("odd length");
        assert!(err.to_string().contains("Hex"));
    }
}

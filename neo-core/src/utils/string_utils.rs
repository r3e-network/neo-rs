//! String conversion utilities to reduce duplication across modules.

use crate::error::{CoreError, CoreResult};

/// Converts bytes to UTF-8 string with consistent error handling.
///
/// This helper reduces duplication of `String::from_utf8().map_err()` patterns
/// throughout the codebase.
///
/// # Examples
///
/// ```rust,ignore
/// use neo_core::utils::bytes_to_utf8_string;
///
/// let bytes = b"hello";
/// let string = bytes_to_utf8_string(bytes)?;
/// assert_eq!(string, "hello");
/// ```
pub fn bytes_to_utf8_string(bytes: &[u8]) -> CoreResult<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| CoreError::serialization(format!("Invalid UTF-8: {}", e)))
}

/// Converts owned bytes to UTF-8 string with consistent error handling.
///
/// This variant consumes the input bytes, avoiding an extra allocation when possible.
pub fn bytes_to_utf8_string_owned(bytes: Vec<u8>) -> CoreResult<String> {
    String::from_utf8(bytes)
        .map_err(|e| CoreError::serialization(format!("Invalid UTF-8: {}", e)))
}

/// Converts bytes to UTF-8 string with a custom error message.
pub fn bytes_to_utf8_string_with_msg(bytes: &[u8], context: &str) -> CoreResult<String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| CoreError::serialization(format!("{}: Invalid UTF-8: {}", context, e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_utf8_conversion() {
        let bytes = b"hello world";
        let result = bytes_to_utf8_string(bytes).unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_invalid_utf8_conversion() {
        let bytes = &[0xFF, 0xFE, 0xFD];
        let result = bytes_to_utf8_string(bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_owned_utf8_conversion() {
        let bytes = b"test".to_vec();
        let result = bytes_to_utf8_string_owned(bytes).unwrap();
        assert_eq!(result, "test");
    }

    #[test]
    fn test_utf8_conversion_with_context() {
        let bytes = &[0xFF, 0xFE];
        let result = bytes_to_utf8_string_with_msg(bytes, "Method call");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Method call"));
    }
}

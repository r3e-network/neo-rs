//! Common validation utilities to reduce duplication across modules.

use crate::error::{CoreError, CoreResult};

/// Maximum input length for native contract methods.
pub const MAX_INPUT_LENGTH: usize = 1024;

/// Validates that args array has at least one element.
///
/// Returns an error if args is empty.
pub fn validate_args_not_empty(args: &[Vec<u8>], method: &str) -> CoreResult<()> {
    if args.is_empty() {
        return Err(CoreError::native_contract(format!(
            "{} requires at least one argument",
            method
        )));
    }
    Ok(())
}

/// Validates that a byte slice does not exceed the maximum input length.
///
/// This is commonly used in native contract implementations to prevent
/// excessive memory usage.
pub fn validate_max_input_length(data: &[u8], method: &str) -> CoreResult<()> {
    validate_max_input_length_custom(data, MAX_INPUT_LENGTH, method)
}

/// Validates that a byte slice does not exceed a custom maximum length.
pub fn validate_max_input_length_custom(
    data: &[u8],
    max_length: usize,
    method: &str,
) -> CoreResult<()> {
    if data.len() > max_length {
        return Err(CoreError::native_contract(format!(
            "{} input exceeds max length {} (got {})",
            method,
            max_length,
            data.len()
        )));
    }
    Ok(())
}

/// Extracts and validates the first argument from an args array.
///
/// This is a common pattern in native contract implementations.
/// Returns a reference to the first argument after validation.
pub fn extract_first_arg<'a>(args: &'a [Vec<u8>], method: &str) -> CoreResult<&'a [u8]> {
    validate_args_not_empty(args, method)?;
    validate_max_input_length(&args[0], method)?;
    Ok(&args[0])
}

/// Extracts and validates the first argument as a UTF-8 string.
///
/// Combines argument extraction, length validation, and UTF-8 conversion.
pub fn extract_first_arg_as_string(args: &[Vec<u8>], method: &str) -> CoreResult<String> {
    let data = extract_first_arg(args, method)?;
    crate::utils::bytes_to_utf8_string_with_msg(data, method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_args_not_empty_success() {
        let args = vec![vec![1, 2, 3]];
        assert!(validate_args_not_empty(&args, "test").is_ok());
    }

    #[test]
    fn test_validate_args_not_empty_failure() {
        let args: Vec<Vec<u8>> = vec![];
        let result = validate_args_not_empty(&args, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test"));
    }

    #[test]
    fn test_validate_max_input_length_success() {
        let data = vec![0u8; 100];
        assert!(validate_max_input_length(&data, "test").is_ok());
    }

    #[test]
    fn test_validate_max_input_length_failure() {
        let data = vec![0u8; MAX_INPUT_LENGTH + 1];
        let result = validate_max_input_length(&data, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_first_arg_success() {
        let args = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let result = extract_first_arg(&args, "test").unwrap();
        assert_eq!(result, &[1, 2, 3]);
    }

    #[test]
    fn test_extract_first_arg_empty() {
        let args: Vec<Vec<u8>> = vec![];
        assert!(extract_first_arg(&args, "test").is_err());
    }

    #[test]
    fn test_extract_first_arg_as_string_success() {
        let args = vec![b"hello".to_vec()];
        let result = extract_first_arg_as_string(&args, "test").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_extract_first_arg_as_string_invalid_utf8() {
        let args = vec![vec![0xFF, 0xFE, 0xFD]];
        assert!(extract_first_arg_as_string(&args, "test").is_err());
    }
}

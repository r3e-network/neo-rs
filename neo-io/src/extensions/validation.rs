//! Validation and `Option`-handling extension traits.
//!
//! These traits previously lived in `neo-core/src/macros.rs` and were moved
//! here because they are defined entirely in terms of [`IoError`] and
//! [`IoResult`], which are owned by this crate.

use crate::{IoError, IoResult};

/// Extension trait for `Option<T>` to simplify error handling with `IoError`.
///
/// # Example
///
/// ```rust
/// use neo_io::IoResult;
/// use neo_io::OptionExt;
///
/// let value: Option<u32> = Some(42);
/// let result: IoResult<u32> = value.ok_or_invalid_data("Value not found");
/// assert_eq!(result.unwrap(), 42);
/// ```
pub trait OptionExt<T> {
    /// Converts `Option<T>` to `IoResult<T>` with an invalid data error message.
    fn ok_or_invalid_data(self, msg: &str) -> IoResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_invalid_data(self, msg: &str) -> IoResult<T> {
        self.ok_or_else(|| IoError::invalid_data(msg))
    }
}

/// Extension trait for validating collection lengths.
///
/// # Example
///
/// ```rust
/// use neo_io::ValidateLength;
///
/// # fn main() -> neo_io::IoResult<()> {
/// let data = vec![1, 2, 3];
/// data.validate_max_length(10, "data")?;
/// # Ok(())
/// # }
/// ```
pub trait ValidateLength {
    /// Validates that the length does not exceed the maximum.
    fn validate_max_length(&self, max: usize, field_name: &str) -> IoResult<()>;
}

impl<T> ValidateLength for Vec<T> {
    fn validate_max_length(&self, max: usize, field_name: &str) -> IoResult<()> {
        if self.len() > max {
            Err(IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for [u8] {
    fn validate_max_length(&self, max: usize, field_name: &str) -> IoResult<()> {
        if self.len() > max {
            Err(IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for str {
    fn validate_max_length(&self, max: usize, field_name: &str) -> IoResult<()> {
        if self.len() > max {
            Err(IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for String {
    fn validate_max_length(&self, max: usize, field_name: &str) -> IoResult<()> {
        self.as_str().validate_max_length(max, field_name)
    }
}

#[cfg(test)]
mod tests {
    use super::{OptionExt, ValidateLength};

    #[test]
    fn test_ok_or_invalid_data_some() {
        let value: Option<u32> = Some(42);
        let result = value.ok_or_invalid_data("Value not found");
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_ok_or_invalid_data_none() {
        let value: Option<u32> = None;
        let result = value.ok_or_invalid_data("Value not found");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Value not found"));
    }

    #[test]
    fn test_validate_max_length_vec_within_limit() {
        let data = vec![1, 2, 3];
        assert!(data.validate_max_length(10, "data").is_ok());
    }

    #[test]
    fn test_validate_max_length_vec_exceeds_limit() {
        let data = vec![1, 2, 3];
        let result = data.validate_max_length(2, "data");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("data"));
    }

    #[test]
    fn test_validate_max_length_slice() {
        let data: &[u8] = &[1, 2, 3];
        assert!(data.validate_max_length(3, "slice").is_ok());
        assert!(data.validate_max_length(2, "slice").is_err());
    }

    #[test]
    fn test_validate_max_length_str_and_string() {
        let field = "cat".to_string();
        assert!(field.validate_max_length(16, "Category").is_ok());
        assert!(field.validate_max_length(2, "Category").is_err());
        let s: &str = &field;
        assert!(s.validate_max_length(16, "Category").is_ok());
        assert!(s.validate_max_length(2, "Category").is_err());
    }
}

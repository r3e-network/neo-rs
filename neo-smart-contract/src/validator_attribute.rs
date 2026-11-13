//! ValidatorAttribute - matches C# Neo.SmartContract.ValidatorAttribute exactly
//!
//! Migrated from `neo-core::smart_contract::validator_attribute`. The only
//! external dependency is `neo_vm::StackValue`, so the module is a verbatim
//! move with no path rewrites required.

use neo_vm::StackValue;

/// Abstract base for validator attributes (matches C# ValidatorAttribute)
/// Note: In C# this is an abstract class with [AttributeUsage(AttributeTargets.Parameter)]
/// In Rust, we implement this as a trait
pub trait ValidatorAttribute: std::fmt::Debug {
    /// Validates a stack item
    fn validate(&self, item: &StackValue) -> Result<(), String>;

    /// Clone the validator
    fn clone_box(&self) -> Box<dyn ValidatorAttribute>;
}

impl Clone for Box<dyn ValidatorAttribute> {
    fn clone(&self) -> Box<dyn ValidatorAttribute> {
        self.clone_box()
    }
}

// Example implementation for a max length validator
/// Validator that rejects byte strings and buffers longer than `max_length`.
#[derive(Clone, Debug)]
pub struct MaxLengthValidator {
    /// The maximum allowed length in bytes.
    pub max_length: usize,
}

impl MaxLengthValidator {
    /// Creates a validator with the given maximum length.
    pub fn new(max_length: usize) -> Self {
        Self { max_length }
    }
}

impl ValidatorAttribute for MaxLengthValidator {
    fn validate(&self, item: &StackValue) -> Result<(), String> {
        match item {
            StackValue::ByteString(bytes) if bytes.len() > self.max_length => Err(format!(
                "ByteString exceeds maximum length of {}",
                self.max_length
            )),
            StackValue::Buffer(buffer) if buffer.len() > self.max_length => Err(format!(
                "Buffer exceeds maximum length of {}",
                self.max_length
            )),
            _ => Ok(()),
        }
    }

    fn clone_box(&self) -> Box<dyn ValidatorAttribute> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm::StackValue;

    #[test]
    fn max_length_validator_accepts_short_bytestring() {
        let validator = MaxLengthValidator::new(10);
        let item = StackValue::ByteString(vec![1, 2, 3]);
        assert!(validator.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_rejects_long_bytestring() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::ByteString(vec![1, 2, 3, 4, 5, 6]);
        assert!(validator.validate(&item).is_err());
    }

    #[test]
    fn max_length_validator_accepts_exact_length_bytestring() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::ByteString(vec![1, 2, 3, 4, 5]);
        assert!(validator.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_accepts_short_buffer() {
        let validator = MaxLengthValidator::new(10);
        let item = StackValue::Buffer(vec![1, 2, 3]);
        assert!(validator.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_rejects_long_buffer() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::Buffer(vec![1, 2, 3, 4, 5, 6]);
        assert!(validator.validate(&item).is_err());
    }

    #[test]
    fn max_length_validator_accepts_non_byte_types() {
        let validator = MaxLengthValidator::new(5);

        assert!(validator.validate(&StackValue::Integer(42.into())).is_ok());
        assert!(validator.validate(&StackValue::Boolean(true)).is_ok());
        assert!(validator.validate(&StackValue::Null).is_ok());
        assert!(validator.validate(&StackValue::Array(vec![])).is_ok());
    }

    #[test]
    fn max_length_validator_clone_box_creates_independent_copy() {
        let validator = MaxLengthValidator::new(10);
        let boxed: Box<dyn ValidatorAttribute> = Box::new(validator.clone());
        let cloned = boxed.clone_box();

        let item = StackValue::ByteString(vec![1, 2, 3]);
        assert!(cloned.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_error_message_includes_limit() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::ByteString(vec![1, 2, 3, 4, 5, 6]);
        let err = validator.validate(&item).unwrap_err();
        assert!(err.contains('5'));
    }

    #[test]
    fn max_length_validator_zero_length_rejects_any_data() {
        let validator = MaxLengthValidator::new(0);
        let item = StackValue::ByteString(vec![1]);
        assert!(validator.validate(&item).is_err());
    }

    #[test]
    fn max_length_validator_accepts_empty_bytestring() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::ByteString(vec![]);
        assert!(validator.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_accepts_empty_buffer() {
        let validator = MaxLengthValidator::new(5);
        let item = StackValue::Buffer(vec![]);
        assert!(validator.validate(&item).is_ok());
    }

    #[test]
    fn max_length_validator_new_sets_correct_limit() {
        let validator = MaxLengthValidator::new(123);
        assert_eq!(validator.max_length, 123);
    }
}

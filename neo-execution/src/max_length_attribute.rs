//! MaxLengthAttribute - matches C# Neo.SmartContract.MaxLengthAttribute exactly

use neo_error::{CoreError, CoreResult};
use neo_manifest::ValidatorAttribute;
use neo_vm_rs::StackValue;

/// MaxLength validator attribute (matches C# MaxLengthAttribute)
#[derive(Clone, Debug)]
pub struct MaxLengthAttribute {
    /// The maximum allowed length
    pub max_length: usize,
}

impl MaxLengthAttribute {
    /// Creates a new MaxLengthAttribute
    pub fn new(max_length: usize) -> Self {
        Self { max_length }
    }
}

impl ValidatorAttribute for MaxLengthAttribute {
    fn validate(&self, item: &StackValue) -> CoreResult<()> {
        let length = match item {
            StackValue::Boolean(_) | StackValue::Integer(_) | StackValue::BigInteger(_) => item
                .to_byte_string_bytes()
                .map(|bytes| bytes.len())
                .unwrap_or(0),
            StackValue::ByteString(bytes) | StackValue::Buffer(_, bytes) => bytes.len(),
            StackValue::Array(_, array) | StackValue::Struct(_, array) => array.len(),
            StackValue::Map(_, map) => map.len(),
            StackValue::Pointer(_) | StackValue::Interop(_) | StackValue::Iterator(_) => 0,
            StackValue::Null => 0,
        };

        if length > self.max_length {
            Err(CoreError::other("The input exceeds the maximum length."))
        } else {
            Ok(())
        }
    }

    fn clone_box(&self) -> Box<dyn ValidatorAttribute> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    #[test]
    fn integer_length_uses_neo_vm_rs_byte_string_rules() {
        let validator = MaxLengthAttribute::new(1);

        assert!(validator.validate(&StackValue::Integer(127)).is_ok());
        assert!(validator.validate(&StackValue::Integer(128)).is_err());
    }

    #[test]
    fn compound_lengths_use_stack_value_shapes() {
        let validator = MaxLengthAttribute::new(1);

        assert!(
            validator
                .validate(&StackValue::Array(0, vec![StackValue::Null]))
                .is_ok()
        );
        assert!(
            validator
                .validate(&StackValue::Struct(
                    0,
                    vec![StackValue::Null, StackValue::Null]
                ))
                .is_err()
        );
        assert!(
            validator
                .validate(&StackValue::Map(
                    0,
                    vec![
                        (StackValue::Integer(1), StackValue::Boolean(true)),
                        (StackValue::Integer(2), StackValue::Boolean(false)),
                    ]
                ))
                .is_err()
        );
    }
}

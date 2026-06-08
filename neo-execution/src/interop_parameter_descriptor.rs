//! InteropParameterDescriptor - matches C# Neo.SmartContract.InteropParameterDescriptor exactly

use neo_manifest::ValidatorAttribute;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

const MAX_INTEGER_SIZE: usize = 32;

/// Type of parameter for interop methods
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteropParameterType {
    /// A generic stack item.
    StackItem,
    /// A pointer type.
    Pointer,
    /// An array type.
    Array,
    /// An interop interface type.
    InteropInterface,
    /// A boolean value.
    Boolean,
    /// A signed byte (-128 to 127).
    SByte,
    /// An unsigned byte (0 to 255).
    Byte,
    /// A signed 16-bit integer.
    Short,
    /// An unsigned 16-bit integer.
    UShort,
    /// A signed 32-bit integer.
    Int,
    /// An unsigned 32-bit integer.
    UInt,
    /// A signed 64-bit integer.
    Long,
    /// An unsigned 64-bit integer.
    ULong,
    /// An arbitrary precision integer.
    BigInteger,
    /// A byte array.
    ByteArray,
    /// A UTF-8 string.
    String,
    /// A 160-bit hash (20 bytes).
    UInt160,
    /// A 256-bit hash (32 bytes).
    UInt256,
    /// An elliptic curve point.
    ECPoint,
}

/// Represents a descriptor of an interoperable service parameter (matches C# InteropParameterDescriptor)
#[derive(Clone, Debug)]
pub struct InteropParameterDescriptor {
    /// The name of the parameter
    pub name: String,

    /// The type of the parameter
    pub param_type: InteropParameterType,

    /// Validators for the parameter
    pub validators: Vec<Box<dyn ValidatorAttribute>>,

    /// Indicates whether null values are allowed for this parameter.
    pub is_nullable: bool,

    /// Indicates whether null values are allowed for array elements.
    pub is_element_nullable: bool,

    /// Indicates whether the parameter is an enumeration
    pub is_enum: bool,

    /// Indicates whether the parameter is an array
    pub is_array: bool,

    /// Indicates whether the parameter is an InteropInterface
    pub is_interface: bool,
}

impl InteropParameterDescriptor {
    /// Creates a new parameter descriptor
    pub fn new(name: String, param_type: InteropParameterType) -> Self {
        let is_array = matches!(param_type, InteropParameterType::Array);
        let is_interface = matches!(param_type, InteropParameterType::InteropInterface);

        Self {
            name,
            param_type,
            validators: Vec::new(),
            is_nullable: false,
            is_element_nullable: false,
            is_enum: false,
            is_array,
            is_interface,
        }
    }

    /// Creates a new parameter descriptor with validators
    pub fn new_with_validators(
        name: String,
        param_type: InteropParameterType,
        validators: Vec<Box<dyn ValidatorAttribute>>,
    ) -> Self {
        let is_array = matches!(param_type, InteropParameterType::Array);
        let is_interface = matches!(param_type, InteropParameterType::InteropInterface);

        Self {
            name,
            param_type,
            validators,
            is_nullable: false,
            is_element_nullable: false,
            is_enum: false,
            is_array,
            is_interface,
        }
    }

    /// Marks whether this parameter accepts null values.
    pub fn with_nullable(mut self, is_nullable: bool) -> Self {
        self.is_nullable = is_nullable;
        self
    }

    /// Marks whether array elements of this parameter accept null values.
    pub fn with_element_nullable(mut self, is_element_nullable: bool) -> Self {
        self.is_element_nullable = is_element_nullable;
        self
    }

    /// Validates a stack item against this parameter descriptor
    pub fn validate(&self, item: &StackValue) -> Result<(), String> {
        // Run all validators
        for validator in &self.validators {
            validator.validate(item)?;
        }
        Ok(())
    }

    /// Converts a stack item to the appropriate type
    pub fn convert(&self, item: &StackValue) -> Result<ConvertedValue, String> {
        if matches!(item, StackValue::Null)
            && !matches!(self.param_type, InteropParameterType::StackItem)
        {
            if !self.is_nullable {
                let name = if self.name.is_empty() {
                    "value"
                } else {
                    &self.name
                };
                return Err(format!("The argument `{name}` can't be null."));
            }
            return Ok(ConvertedValue::Null);
        }

        match self.param_type {
            InteropParameterType::StackItem => Ok(ConvertedValue::StackValue(item.clone())),
            InteropParameterType::Boolean => match item {
                StackValue::Boolean(value) => Ok(ConvertedValue::Boolean(*value)),
                _ => Err("Expected boolean".to_string()),
            },
            InteropParameterType::Int => {
                let integer = stack_value_bigint(item)?;
                integer
                    .to_i32()
                    .map(ConvertedValue::Int)
                    .ok_or_else(|| "Integer out of range".to_string())
            }
            InteropParameterType::String => match item {
                StackValue::Null => Ok(ConvertedValue::String(String::new())),
                StackValue::ByteString(bytes) => String::from_utf8(bytes.clone())
                    .map(ConvertedValue::String)
                    .map_err(|_| "Invalid UTF-8 string".to_string()),
                StackValue::Buffer(buffer) => String::from_utf8(buffer.clone())
                    .map(ConvertedValue::String)
                    .map_err(|_| "Invalid UTF-8 string".to_string()),
                _ => Err("Expected string".to_string()),
            },
            InteropParameterType::ByteArray => match item {
                StackValue::Null => Ok(ConvertedValue::ByteArray(Vec::new())),
                StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
                    Ok(ConvertedValue::ByteArray(bytes.clone()))
                }
                _ => Err("Expected byte array".to_string()),
            },
            InteropParameterType::UInt160 => match item {
                StackValue::Null => Ok(ConvertedValue::UInt160(neo_primitives::UInt160::default())),
                StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() == 20 => {
                    let mut arr = [0u8; 20];
                    arr.copy_from_slice(bytes);
                    let value = neo_primitives::UInt160::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt160(value))
                }
                _ => Err("Expected UInt160".to_string()),
            },
            InteropParameterType::UInt256 => match item {
                StackValue::Null => Ok(ConvertedValue::UInt256(neo_primitives::UInt256::default())),
                StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    let value = neo_primitives::UInt256::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt256(value))
                }
                _ => Err("Expected UInt256".to_string()),
            },
            _ => Ok(ConvertedValue::StackValue(item.clone())),
        }
    }
}

fn stack_value_bigint(item: &StackValue) -> Result<BigInt, String> {
    match item {
        StackValue::Boolean(value) => Ok(BigInt::from(i32::from(*value))),
        StackValue::Integer(value) => Ok(BigInt::from(*value)),
        StackValue::BigInteger(bytes)
        | StackValue::ByteString(bytes)
        | StackValue::Buffer(bytes) => {
            if bytes.len() > MAX_INTEGER_SIZE {
                return Err("Expected integer".to_string());
            }
            Ok(BigInt::from_signed_bytes_le(bytes))
        }
        _ => Err("Expected integer".to_string()),
    }
}

/// Converted value from stack item
#[derive(Clone, Debug)]
pub enum ConvertedValue {
    /// A null value preserved for nullable interop parameters.
    Null,
    /// A generic stack item.
    StackValue(StackValue),
    /// A boolean value.
    Boolean(bool),
    /// A 32-bit integer.
    Int(i32),
    /// A UTF-8 string.
    String(String),
    /// A byte array.
    ByteArray(Vec<u8>),
    /// A 160-bit hash.
    UInt160(neo_primitives::UInt160),
    /// A 256-bit hash.
    UInt256(neo_primitives::UInt256),
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    #[test]
    fn non_nullable_string_rejects_null() {
        let descriptor =
            InteropParameterDescriptor::new("value".to_string(), InteropParameterType::String);

        let err = descriptor
            .convert(&StackValue::Null)
            .expect_err("non-nullable strings must reject null");

        assert!(err.contains("null"));
    }

    #[test]
    fn converts_primitive_stack_values_without_local_stack_item() {
        let int_descriptor =
            InteropParameterDescriptor::new("value".to_string(), InteropParameterType::Int);
        assert!(matches!(
            int_descriptor
                .convert(&StackValue::BigInteger(vec![42]))
                .unwrap(),
            ConvertedValue::Int(42)
        ));

        let string_descriptor =
            InteropParameterDescriptor::new("value".to_string(), InteropParameterType::String);
        assert!(matches!(
            string_descriptor
                .convert(&StackValue::ByteString(b"neo".to_vec()))
                .unwrap(),
            ConvertedValue::String(value) if value == "neo"
        ));

        let generic_descriptor =
            InteropParameterDescriptor::new("value".to_string(), InteropParameterType::StackItem);
        assert!(matches!(
            generic_descriptor
                .convert(&StackValue::Array(vec![StackValue::Boolean(true)]))
                .unwrap(),
            ConvertedValue::StackValue(StackValue::Array(values)) if values == vec![StackValue::Boolean(true)]
        ));
    }
}

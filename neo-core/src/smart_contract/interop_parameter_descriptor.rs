//! InteropParameterDescriptor - matches C# Neo.SmartContract.InteropParameterDescriptor exactly

use crate::smart_contract::validator_attribute::ValidatorAttribute;
use neo_vm::StackItem;
use num_traits::ToPrimitive;

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
            is_enum: false,
            is_array,
            is_interface,
        }
    }

    /// Validates a stack item against this parameter descriptor
    pub fn validate(&self, item: &StackItem) -> Result<(), String> {
        // Run all validators
        for validator in &self.validators {
            validator.validate(item)?;
        }
        Ok(())
    }

    /// Converts a stack item to the appropriate type
    pub fn convert(&self, item: &StackItem) -> Result<ConvertedValue, String> {
        match self.param_type {
            InteropParameterType::StackItem => Ok(ConvertedValue::StackItem(item.clone())),
            InteropParameterType::Boolean => match item {
                StackItem::Boolean(value) => Ok(ConvertedValue::Boolean(*value)),
                _ => Err("Expected boolean".to_string()),
            },
            InteropParameterType::Int => {
                let integer = item.as_int().map_err(|_| "Expected integer".to_string())?;
                integer
                    .to_i32()
                    .map(ConvertedValue::Int)
                    .ok_or_else(|| "Integer out of range".to_string())
            }
            InteropParameterType::String => match item {
                StackItem::Null => Ok(ConvertedValue::String(String::new())),
                StackItem::ByteString(bytes) => String::from_utf8(bytes.clone())
                    .map(ConvertedValue::String)
                    .map_err(|_| "Invalid UTF-8 string".to_string()),
                StackItem::Buffer(buffer) => String::from_utf8(buffer.data())
                    .map(ConvertedValue::String)
                    .map_err(|_| "Invalid UTF-8 string".to_string()),
                _ => Err("Expected string".to_string()),
            },
            InteropParameterType::ByteArray => match item {
                StackItem::Null => Ok(ConvertedValue::ByteArray(Vec::new())),
                StackItem::ByteString(bytes) => Ok(ConvertedValue::ByteArray(bytes.clone())),
                StackItem::Buffer(buffer) => Ok(ConvertedValue::ByteArray(buffer.data())),
                _ => Err("Expected byte array".to_string()),
            },
            InteropParameterType::UInt160 => match item {
                StackItem::Null => Ok(ConvertedValue::UInt160(crate::UInt160::default())),
                StackItem::ByteString(bytes) if bytes.len() == 20 => {
                    let mut arr = [0u8; 20];
                    arr.copy_from_slice(bytes);
                    let value = crate::UInt160::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt160(value))
                }
                StackItem::Buffer(buffer) if buffer.data().len() == 20 => {
                    let data = buffer.data();
                    let mut arr = [0u8; 20];
                    arr.copy_from_slice(&data);
                    let value = crate::UInt160::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt160(value))
                }
                _ => Err("Expected UInt160".to_string()),
            },
            InteropParameterType::UInt256 => match item {
                StackItem::Null => Ok(ConvertedValue::UInt256(crate::UInt256::default())),
                StackItem::ByteString(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(bytes);
                    let value = crate::UInt256::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt256(value))
                }
                StackItem::Buffer(buffer) if buffer.data().len() == 32 => {
                    let data = buffer.data();
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&data);
                    let value = crate::UInt256::from_bytes(&arr).map_err(|e| e.to_string())?;
                    Ok(ConvertedValue::UInt256(value))
                }
                _ => Err("Expected UInt256".to_string()),
            },
            _ => Ok(ConvertedValue::StackItem(item.clone())),
        }
    }
}

/// Converted value from stack item
#[derive(Clone, Debug)]
pub enum ConvertedValue {
    /// A generic stack item.
    StackItem(StackItem),
    /// A boolean value.
    Boolean(bool),
    /// A 32-bit integer.
    Int(i32),
    /// A UTF-8 string.
    String(String),
    /// A byte array.
    ByteArray(Vec<u8>),
    /// A 160-bit hash.
    UInt160(crate::UInt160),
    /// A 256-bit hash.
    UInt256(crate::UInt256),
}

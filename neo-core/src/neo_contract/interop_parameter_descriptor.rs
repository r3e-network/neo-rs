use std::convert::TryFrom;
use std::sync::Arc;
use neo_vm::vm_types::stack_item::StackItem;
use crate::cryptography::ECPoint;
use crate::neo_contract::validator_attribute::ValidatorTrait;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// Represents a descriptor of an interoperable service parameter.
#[derive(Debug, Clone)]
pub struct InteropParameterDescriptor {
    validators: Vec<dyn ValidatorTrait>,
    /// The name of the parameter.
    pub name: String,
    /// The type of the parameter.
    pub param_type: InteropParameterType,
    /// The converter to convert the parameter from `StackItem` to the target type.
    pub converter: Arc<dyn Fn(&StackItem) -> Result<Box<dyn std::any::Any>, String>>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum InteropParameterType {
    StackItem,
    Pointer,
    Array,
    InteropInterface,
    Boolean,
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    BigInt,
    ByteArray,
    String,
    UInt160,
    UInt256,
    ECPoint,
    Enum(String),
    CustomArray(Box<InteropParameterType>),
}

impl InteropParameterDescriptor {
    pub fn new(name: String, param_type: InteropParameterType, validators: Vec<dyn ValidatorTrait>) -> Self {
        let converter = Arc::new(Self::get_converter(&param_type));
        Self {
            validators,
            name,
            param_type,
            converter,
        }
    }

    fn get_converter(param_type: &InteropParameterType) -> impl Fn(&StackItem) -> Result<Box<dyn std::any::Any>, String> {
        match param_type {
            InteropParameterType::StackItem => |p| Ok(Box::new(p.clone())),
            InteropParameterType::Pointer => |p| Ok(Box::new(p.clone())),
            InteropParameterType::Array => |p| Ok(Box::new(p.clone())),
            InteropParameterType::InteropInterface => |p| Ok(Box::new(p.clone())),
            InteropParameterType::Boolean => |p| Ok(Box::new(p.get_boolean()?)),
            InteropParameterType::I8 => |p| Ok(Box::new(i8::try_from(p.get_integer()?)?)),
            InteropParameterType::U8 => |p| Ok(Box::new(u8::try_from(p.get_integer()?)?)),
            InteropParameterType::I16 => |p| Ok(Box::new(i16::try_from(p.get_integer()?)?)),
            InteropParameterType::U16 => |p| Ok(Box::new(u16::try_from(p.get_integer()?)?)),
            InteropParameterType::I32 => |p| Ok(Box::new(i32::try_from(p.get_integer()?)?)),
            InteropParameterType::U32 => |p| Ok(Box::new(u32::try_from(p.get_integer()?)?)),
            InteropParameterType::I64 => |p| Ok(Box::new(i64::try_from(p.get_integer()?)?)),
            InteropParameterType::U64 => |p| Ok(Box::new(u64::try_from(p.get_integer()?)?)),
            InteropParameterType::BigInt => |p| Ok(Box::new(p.get_integer()?)),
            InteropParameterType::ByteArray => |p| {
                if p.is_null() {
                    Ok(Box::new(None::<Vec<u8>>))
                } else {
                    Ok(Box::new(Some(p.get_span()?.to_vec())))
                }
            },
            InteropParameterType::String => |p| {
                if p.is_null() {
                    Ok(Box::new(None::<String>))
                } else {
                    Ok(Box::new(Some(p.get_string()?)))
                }
            },
            InteropParameterType::UInt160 => |p| {
                if p.is_null() {
                    Ok(Box::new(None::<UInt160>))
                } else {
                    Ok(Box::new(Some(UInt160::try_from(p.get_span()?)?)))
                }
            },
            InteropParameterType::UInt256 => |p| {
                if p.is_null() {
                    Ok(Box::new(None::<UInt256>))
                } else {
                    Ok(Box::new(Some(UInt256::try_from(p.get_span()?)?)))
                }
            },
            InteropParameterType::ECPoint => |p| {
                if p.is_null() {
                    Ok(Box::new(None::<ECPoint>))
                } else {
                    Ok(Box::new(Some(ECPoint::from_encoded(p.get_span()?))))
                }
            },
            InteropParameterType::Enum(_) | InteropParameterType::CustomArray(_) => unimplemented!("Enum and CustomArray types are not yet implemented"),
        }
    }

    pub fn validate(&self, item: &StackItem) -> Result<(), String> {
        for validator in &self.validators {
            validator.validate(item)?;
        }
        Ok(())
    }
}

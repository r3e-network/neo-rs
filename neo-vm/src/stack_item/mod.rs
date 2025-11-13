use alloc::vec::Vec;

use neo_base::{Bytes, DecodeError, NeoRead, SliceReader};

use crate::{error::VmError, value::VmValue};

const MAX_STACK_ITEMS: usize = 2048;
const MAX_ITEM_SIZE: u32 = u16::MAX as u32 * 2;
const MAX_INTEGER_SIZE: usize = 32;

/// Minimal StackItem model used to bridge VM values with higher-level
/// serialization (BinarySerializer) logic.
#[derive(Clone, Debug, PartialEq)]
pub enum StackItem {
    Null,
    Boolean(bool),
    Integer(i128),
    ByteString(Bytes),
    Array(Vec<StackItem>),
    Struct(Vec<StackItem>),
}

#[repr(u8)]
enum StackItemType {
    Any = 0x00,
    Boolean = 0x20,
    Integer = 0x21,
    ByteString = 0x28,
    Array = 0x40,
    Struct = 0x41,
}

impl TryFrom<u8> for StackItemType {
    type Error = VmError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(StackItemType::Any),
            0x20 => Ok(StackItemType::Boolean),
            0x21 => Ok(StackItemType::Integer),
            0x28 => Ok(StackItemType::ByteString),
            0x40 => Ok(StackItemType::Array),
            0x41 => Ok(StackItemType::Struct),
            _ => Err(VmError::InvalidType),
        }
    }
}

impl StackItem {
    pub fn null() -> Self {
        StackItem::Null
    }

    pub fn boolean(value: bool) -> Self {
        StackItem::Boolean(value)
    }

    pub fn integer(value: impl Into<i128>) -> Self {
        StackItem::Integer(value.into())
    }

    pub fn byte_string(bytes: Bytes) -> Self {
        StackItem::ByteString(bytes)
    }

    pub fn array(values: Vec<StackItem>) -> Self {
        StackItem::Array(values)
    }

    pub fn r#struct(values: Vec<StackItem>) -> Self {
        StackItem::Struct(values)
    }

    pub fn from_vm_value(value: VmValue) -> Result<Self, VmError> {
        match value {
            VmValue::Null => Ok(StackItem::Null),
            VmValue::Bool(v) => Ok(StackItem::Boolean(v)),
            VmValue::Int(v) => Ok(StackItem::Integer(v as i128)),
            VmValue::Bytes(bytes) => Ok(StackItem::ByteString(bytes)),
            VmValue::String(s) => Ok(StackItem::ByteString(Bytes::from(s.into_bytes()))),
            VmValue::Array(values) => {
                let mut items = Vec::with_capacity(values.len());
                for value in values {
                    items.push(StackItem::from_vm_value(value)?);
                }
                Ok(StackItem::Array(items))
            }
        }
    }

    pub fn try_into_vm_value(self) -> Result<VmValue, VmError> {
        match self {
            StackItem::Null => Ok(VmValue::Null),
            StackItem::Boolean(v) => Ok(VmValue::Bool(v)),
            StackItem::Integer(v) => {
                if v > i64::MAX as i128 || v < i64::MIN as i128 {
                    return Err(VmError::InvalidType);
                }
                Ok(VmValue::Int(v as i64))
            }
            StackItem::ByteString(bytes) => Ok(VmValue::Bytes(bytes)),
            StackItem::Array(values) | StackItem::Struct(values) => {
                let mut converted = Vec::with_capacity(values.len());
                for value in values {
                    converted.push(value.try_into_vm_value()?);
                }
                Ok(VmValue::Array(converted))
            }
        }
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, VmError> {
        let mut reader = SliceReader::new(data);
        let mut remaining = MAX_STACK_ITEMS;
        let item = Self::deserialize_with_limits(&mut reader, &mut remaining)?;
        if reader.remaining() != 0 {
            return Err(VmError::InvalidType);
        }
        Ok(item)
    }

    fn deserialize_with_limits(
        reader: &mut SliceReader<'_>,
        remaining: &mut usize,
    ) -> Result<Self, VmError> {
        if *remaining == 0 {
            return Err(VmError::InvalidType);
        }
        *remaining -= 1;
        let ty = StackItemType::try_from(read_byte(reader)?)?;
        match ty {
            StackItemType::Any => Ok(StackItem::Null),
            StackItemType::Boolean => Ok(StackItem::Boolean(read_bool(reader)?)),
            StackItemType::Integer => {
                let bytes = reader
                    .read_var_bytes(MAX_INTEGER_SIZE as u64)
                    .map_err(map_decode_error)?;
                Ok(StackItem::Integer(decode_integer(&bytes)?))
            }
            StackItemType::ByteString => Ok(StackItem::ByteString(Bytes::from(
                reader
                    .read_var_bytes(MAX_ITEM_SIZE as u64)
                    .map_err(map_decode_error)?,
            ))),
            StackItemType::Array | StackItemType::Struct => {
                let count = reader
                    .read_varint()
                    .map_err(map_decode_error)?
                    .try_into()
                    .map_err(|_| VmError::InvalidType)?;
                let count: usize = count;
                if count > *remaining {
                    return Err(VmError::InvalidType);
                }
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(Self::deserialize_with_limits(reader, remaining)?);
                }
                Ok(if matches!(ty, StackItemType::Struct) {
                    StackItem::Struct(values)
                } else {
                    StackItem::Array(values)
                })
            }
        }
    }
}

impl TryFrom<VmValue> for StackItem {
    type Error = VmError;

    fn try_from(value: VmValue) -> Result<Self, Self::Error> {
        StackItem::from_vm_value(value)
    }
}

impl TryFrom<StackItem> for VmValue {
    type Error = VmError;

    fn try_from(item: StackItem) -> Result<Self, Self::Error> {
        item.try_into_vm_value()
    }
}

fn read_byte(reader: &mut SliceReader<'_>) -> Result<u8, VmError> {
    reader.read_u8().map_err(map_decode_error)
}

fn read_bool(reader: &mut SliceReader<'_>) -> Result<bool, VmError> {
    match read_byte(reader)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(VmError::InvalidType),
    }
}

fn decode_integer(bytes: &[u8]) -> Result<i128, VmError> {
    if bytes.len() > 16 {
        return Err(VmError::InvalidType);
    }
    if bytes.is_empty() {
        return Ok(0);
    }
    let sign_extend = bytes.last().copied().unwrap_or(0) & 0x80 != 0;
    let mut buf = if sign_extend { [0xFFu8; 16] } else { [0u8; 16] };
    buf[..bytes.len()].copy_from_slice(bytes);
    Ok(i128::from_le_bytes(buf))
}

fn map_decode_error(_err: DecodeError) -> VmError {
    VmError::InvalidType
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use neo_base::Bytes;

    use super::StackItem;
    use crate::value::VmValue;

    #[test]
    fn converts_scalar_items() {
        let cases = vec![
            (StackItem::Null, VmValue::Null),
            (StackItem::Boolean(true), VmValue::Bool(true)),
            (StackItem::Integer(42), VmValue::Int(42)),
            (
                StackItem::ByteString(Bytes::from(&b"hello"[..])),
                VmValue::Bytes(Bytes::from(&b"hello"[..])),
            ),
        ];

        for (item, value) in cases {
            assert_eq!(item.clone().try_into_vm_value().unwrap(), value);
            assert_eq!(StackItem::from_vm_value(value).unwrap(), item);
        }
    }

    #[test]
    fn converts_arrays() {
        let nested = StackItem::array(vec![StackItem::Boolean(true), StackItem::Integer(3)]);
        let vm = nested.clone().try_into_vm_value().unwrap();
        if let VmValue::Array(values) = vm {
            assert_eq!(values.len(), 2);
            assert_eq!(values[0], VmValue::Bool(true));
            assert_eq!(values[1], VmValue::Int(3));
            let roundtrip = StackItem::from_vm_value(VmValue::Array(values)).unwrap();
            assert_eq!(
                roundtrip,
                StackItem::array(vec![StackItem::Boolean(true), StackItem::Integer(3)])
            );
        } else {
            panic!("expected array VmValue");
        }
    }
}

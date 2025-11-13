use alloc::vec::Vec;

use neo_base::Bytes;

use crate::{error::VmError, value::VmValue};

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

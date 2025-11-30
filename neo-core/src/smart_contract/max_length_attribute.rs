//! MaxLengthAttribute - matches C# Neo.SmartContract.MaxLengthAttribute exactly

use crate::smart_contract::validator_attribute::ValidatorAttribute;
use neo_vm::StackItem;

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
    fn validate(&self, item: &StackItem) -> Result<(), String> {
        let length = match item {
            StackItem::Boolean(_) => 1,
            StackItem::Integer(_) => item.as_bytes().map(|bytes| bytes.len()).unwrap_or(0),
            StackItem::ByteString(bytes) => bytes.len(),
            StackItem::Buffer(buffer) => buffer.data().len(),
            StackItem::Array(array) => array.len(),
            StackItem::Struct(struct_item) => struct_item.len(),
            StackItem::Map(map) => map.len(),
            StackItem::Pointer(_) => 0,
            StackItem::InteropInterface(_) => 0,
            StackItem::Null => 0,
        };

        if length > self.max_length {
            Err("The input exceeds the maximum length.".to_string())
        } else {
            Ok(())
        }
    }

    fn clone_box(&self) -> Box<dyn ValidatorAttribute> {
        Box::new(self.clone())
    }
}

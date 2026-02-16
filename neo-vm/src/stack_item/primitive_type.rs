//! Helper views for primitive stack items.
//!
//! Mirrors the behaviour of `Neo.VM/Types/PrimitiveType.cs` by providing
//! convenience accessors for stack items that represent primitive values
//! (integers, booleans, byte strings and buffers).

use crate::error::{VmError, VmResult};
use crate::stack_item::StackItem;
use crate::stack_item::stack_item_type::StackItemType;
use num_bigint::BigInt;

/// Read-only view over a primitive stack item.
#[derive(Debug, Clone, Copy)]
pub struct PrimitiveType<'a> {
    item: &'a StackItem,
}

impl<'a> PrimitiveType<'a> {
    /// Attempts to wrap the provided stack item, returning `None` if it is not a primitive type.
    #[must_use]
    pub fn new(item: &'a StackItem) -> Option<Self> {
        matches!(
            item.stack_item_type(),
            StackItemType::Boolean
                | StackItemType::Integer
                | StackItemType::ByteString
                | StackItemType::Buffer
        )
        .then_some(Self { item })
    }

    /// Returns the raw memory for the primitive item.
    pub fn memory(&self) -> VmResult<Vec<u8>> {
        self.item.as_bytes()
    }

    /// Returns the size (in bytes) of the primitive item.
    pub fn size(&self) -> VmResult<usize> {
        Ok(self.memory()?.len())
    }

    /// Converts the primitive to a boolean following the Neo VM rules.
    pub fn get_boolean(&self) -> VmResult<bool> {
        self.item.as_bool()
    }

    /// Converts the primitive to an integer following the Neo VM rules.
    pub fn get_integer(&self) -> VmResult<BigInt> {
        self.item.as_int()
    }

    /// Converts the primitive to another primitive stack item type.
    pub fn convert_to(&self, target: StackItemType) -> VmResult<StackItem> {
        match target {
            StackItemType::Boolean
            | StackItemType::Integer
            | StackItemType::ByteString
            | StackItemType::Buffer => self.item.convert_to(target),
            _ => Err(VmError::invalid_type_simple(format!(
                "Cannot convert primitive to {target:?}"
            ))),
        }
    }

    /// Returns the underlying stack item.
    #[must_use]
    pub const fn as_item(&self) -> &'a StackItem {
        self.item
    }
}

impl<'a> From<PrimitiveType<'a>> for &'a StackItem {
    fn from(value: PrimitiveType<'a>) -> Self {
        value.item
    }
}

/// Helper trait to obtain a primitive view from a stack item.
pub trait PrimitiveTypeExt {
    /// Returns a primitive view or an error if the item is not primitive.
    fn as_primitive(&self) -> VmResult<PrimitiveType<'_>>;
}

impl PrimitiveTypeExt for StackItem {
    fn as_primitive(&self) -> VmResult<PrimitiveType<'_>> {
        PrimitiveType::new(self)
            .ok_or_else(|| VmError::invalid_type_simple("Stack item is not a primitive type"))
    }
}

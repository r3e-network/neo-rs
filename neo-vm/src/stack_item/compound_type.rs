//! Helper views for compound stack items (arrays, structs and maps).
//!
//! Ports the public surface of `Neo.VM/Types/CompoundType.cs` to Rust. The view
//! is intentionally lightweight – it does not own the underlying stack item, it
//! simply offers convenient accessors that mirror the behaviour of the C# base
//! class used by `Array`, `Struct` and `Map`.

use crate::error::{VmError, VmResult};
use crate::stack_item::StackItem;
use crate::stack_item::StackItemType;

/// Read-only view over a compound stack item.
#[derive(Debug, Clone, Copy)]
pub struct CompoundType<'a> {
    item: &'a StackItem,
}

impl<'a> CompoundType<'a> {
    /// Attempts to wrap the supplied stack item.
    #[must_use] 
    pub fn new(item: &'a StackItem) -> Option<Self> {
        matches!(
            item.stack_item_type(),
            StackItemType::Array | StackItemType::Struct | StackItemType::Map
        )
        .then_some(Self { item })
    }

    /// Returns the number of elements contained in the compound item.
    #[must_use] 
    pub fn count(&self) -> usize {
        match self.item {
            StackItem::Array(array) => array.len(),
            StackItem::Struct(structure) => structure.len(),
            StackItem::Map(map) => map.len(),
            _ => 0,
        }
    }

    /// Enumerates the child stack items. For maps this yields the values, matching the
    /// semantics used by the C# implementation when tracking references.
    #[must_use] 
    pub fn sub_items(&self) -> Vec<StackItem> {
        match self.item {
            StackItem::Array(array) => array.iter().collect(),
            StackItem::Struct(structure) => structure.items(),
            StackItem::Map(map) => map.items().values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Indicates whether the compound item is read-only. The Neo VM treats raw
    /// `Array`, `Struct` and `Map` stack items as mutable by default.
    #[must_use] 
    pub const fn is_read_only(&self) -> bool {
        false
    }

    /// Returns the underlying stack item.
    #[must_use] 
    pub const fn as_item(&self) -> &'a StackItem {
        self.item
    }
}

/// Mutable view over a compound stack item.

#[derive(Debug)]
pub struct CompoundTypeMut<'a> {
    item: &'a mut StackItem,
}

impl<'a> CompoundTypeMut<'a> {
    /// Attempts to wrap the supplied stack item mutably.
    pub fn new(item: &'a mut StackItem) -> Option<Self> {
        matches!(
            item.stack_item_type(),
            StackItemType::Array | StackItemType::Struct | StackItemType::Map
        )
        .then_some(Self { item })
    }

    /// Removes every element from the compound object.
    pub fn clear(&mut self) {
        match self.item {
            StackItem::Array(array) => {
                let _ = array.clear();
            }
            StackItem::Struct(structure) => {
                let _ = structure.clear();
            }
            StackItem::Map(map) => {
                let _ = map.clear();
            }
            _ => {}
        }
    }

    // NOTE: Direct mutable access to compound items is no longer exposed here.
}

/// Helper extension trait to obtain a compound view.
pub trait CompoundTypeExt {
    fn as_compound(&self) -> VmResult<CompoundType<'_>>;
    fn as_compound_mut(&mut self) -> VmResult<CompoundTypeMut<'_>>;
}

impl CompoundTypeExt for StackItem {
    fn as_compound(&self) -> VmResult<CompoundType<'_>> {
        CompoundType::new(self)
            .ok_or_else(|| VmError::invalid_type_simple("Stack item is not a compound type"))
    }

    fn as_compound_mut(&mut self) -> VmResult<CompoundTypeMut<'_>> {
        CompoundTypeMut::new(self)
            .ok_or_else(|| VmError::invalid_type_simple("Stack item is not a compound type"))
    }
}

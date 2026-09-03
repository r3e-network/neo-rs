//! Stack item module for the Neo Virtual Machine.
//!
//! This module provides the stack item types used in the Neo VM.

/// Array stack item type.
pub mod array;
/// Buffer stack item type.
pub mod buffer;
/// Map stack item type.
pub mod map;
/// Pointer stack item type.
pub mod pointer;
/// Core stack item enum and operations.
#[allow(clippy::module_inception)]
pub mod stack_item;
/// Struct stack item type.
pub mod struct_item;
/// VM integer stack item.
pub mod vm_integer;

pub use array::Array;
pub use buffer::Buffer;
pub use map::Map;
pub use pointer::Pointer;
pub use stack_item::InteropInterface;
pub use stack_item::StackItem;
pub use struct_item::Struct;
pub use vm_integer::VmInteger;

/// Backward-compatibility extension trait. All operations historically provided
/// by `StackItemExt` are inherent methods on [`StackItem`]; this empty marker
/// trait is exported so existing `use ...StackItemExt;` imports keep resolving.
pub trait StackItemExt {}
impl StackItemExt for StackItem {}

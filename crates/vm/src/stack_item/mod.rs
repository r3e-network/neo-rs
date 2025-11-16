//! Stack item module for the Neo Virtual Machine.
//!
//! This module provides the stack item types used in the Neo VM.

pub mod array;
pub mod boolean;
pub mod buffer;
pub mod byte_string;
pub mod compound_type;
pub mod integer;
pub mod interop_interface;
pub mod map;
pub mod null;
pub mod pointer;
pub mod primitive_type;
#[allow(clippy::module_inception)]
pub mod stack_item;
pub mod stack_item_type;
pub mod stack_item_vertex; // allow module inception for clarity of type name
pub mod struct_item;

pub use array::Array;
pub use boolean::Boolean;
pub use buffer::Buffer;
pub use byte_string::ByteString;
pub use compound_type::{CompoundType, CompoundTypeExt, CompoundTypeMut};
pub use integer::Integer;
pub use map::Map;
pub use null::Null;
pub use pointer::Pointer;
pub use primitive_type::{PrimitiveType, PrimitiveTypeExt};
pub use stack_item::InteropInterface;
pub use stack_item::StackItem;
pub use stack_item_type::StackItemType;
pub use stack_item_vertex::next_stack_item_id;
pub use struct_item::Struct;

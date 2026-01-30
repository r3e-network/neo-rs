//! Stack item module for the Neo Virtual Machine.
//!
//! This module provides the stack item types used in the Neo VM.

/// Array stack item type.
pub mod array;
/// Boolean stack item type.
pub mod boolean;
/// Buffer stack item type.
pub mod buffer;
/// Byte string stack item type.
pub mod byte_string;
/// Compound type trait and implementations.
pub mod compound_type;
/// Integer stack item type.
pub mod integer;
/// Interop interface stack item type.
pub mod interop_interface;
/// Map stack item type.
pub mod map;
/// Null stack item type.
pub mod null;
/// Pointer stack item type.
pub mod pointer;
/// Primitive type trait.
pub mod primitive_type;
/// Core stack item enum and operations.
#[allow(clippy::module_inception)]
pub mod stack_item;
/// Stack item type enumeration.
pub mod stack_item_type;
/// Stack item vertex for graph operations.
pub mod stack_item_vertex;
/// Struct stack item type.
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

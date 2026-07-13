//! # neo-vm::stack_item
//!
//! NeoVM stack item representations and conversion helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `array`: VM array stack item implementation.
//! - `buffer`: VM buffer stack item implementation.
//! - `map`: VM map stack item implementation.
//! - `pointer`: VM pointer stack item implementation.
//! - `stack_item`: NeoVM stack item representations and conversion helpers.
//! - `stack_value_conversion`: Alias-preserving external graph conversion.
//! - `struct_item`: VM struct stack item implementation.
//! - `vm_integer`: VM integer stack item implementation.

/// Array stack item type.
pub mod array;
/// Buffer stack item type.
pub mod buffer;
/// Map stack item type.
pub mod map;
/// Pointer stack item type.
pub mod pointer;
/// Core stack item enum and operations.
// Rationale: the nested module name mirrors the C# StackItem domain while this
// root module remains the facade for stack item subtypes.
#[allow(clippy::module_inception)]
pub mod stack_item;
mod stack_value_conversion;
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

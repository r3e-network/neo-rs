//! Stack item type definitions for the Neo Virtual Machine.
//!
//! This module defines the types of items that can be stored on the VM stack.

/// Represents the types in the VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum StackItemType {
    /// Represents any type.
    Any = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY,

    /// Represents a code pointer.
    Pointer = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_POINTER,

    /// Represents the boolean (true or false) type.
    Boolean = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN,

    /// Represents an integer.
    Integer = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER,

    /// Represents an immutable memory block.
    ByteString = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING,

    /// Represents a memory block that can be used for reading and writing.
    Buffer = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BUFFER,

    /// Represents an array or a complex object.
    Array = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY,

    /// Represents a structure.
    Struct = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT,

    /// Represents an ordered collection of key-value pairs.
    Map = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_MAP,

    /// Represents an interface used to interoperate with the outside of the VM.
    InteropInterface = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
}

impl StackItemType {
    /// Converts a byte to a `StackItemType`.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        match b {
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY => Some(Self::Any),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_POINTER => Some(Self::Pointer),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN => Some(Self::Boolean),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER => Some(Self::Integer),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING => Some(Self::ByteString),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BUFFER => Some(Self::Buffer),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY => Some(Self::Array),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT => Some(Self::Struct),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_MAP => Some(Self::Map),
            neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE => Some(Self::InteropInterface),
            _ => None,
        }
    }

    /// Converts a `StackItemType` to a byte.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Any => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ANY,
            Self::Pointer => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_POINTER,
            Self::Boolean => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BOOLEAN,
            Self::Integer => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER,
            Self::ByteString => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING,
            Self::Buffer => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BUFFER,
            Self::Array => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY,
            Self::Struct => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT,
            Self::Map => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_MAP,
            Self::InteropInterface => neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
        }
    }
}

//! Canonical NeoVM stack-item type tags.

/// NeoVM `StackItemType.Any`.
pub const NEOVM_STACK_ITEM_TYPE_ANY: u8 = 0x00;
/// NeoVM `StackItemType.Pointer`.
pub const NEOVM_STACK_ITEM_TYPE_POINTER: u8 = 0x10;
/// NeoVM `StackItemType.Boolean`.
pub const NEOVM_STACK_ITEM_TYPE_BOOLEAN: u8 = 0x20;
/// NeoVM `StackItemType.Integer`.
pub const NEOVM_STACK_ITEM_TYPE_INTEGER: u8 = 0x21;
/// NeoVM `StackItemType.ByteString`.
pub const NEOVM_STACK_ITEM_TYPE_BYTESTRING: u8 = 0x28;
/// NeoVM `StackItemType.Buffer`.
pub const NEOVM_STACK_ITEM_TYPE_BUFFER: u8 = 0x30;
/// NeoVM `StackItemType.Array`.
pub const NEOVM_STACK_ITEM_TYPE_ARRAY: u8 = 0x40;
/// NeoVM `StackItemType.Struct`.
pub const NEOVM_STACK_ITEM_TYPE_STRUCT: u8 = 0x41;
/// NeoVM `StackItemType.Map`.
pub const NEOVM_STACK_ITEM_TYPE_MAP: u8 = 0x48;
/// NeoVM `StackItemType.InteropInterface`.
pub const NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE: u8 = 0x60;

/// C# Neo.VM stack-item type tags.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StackItemType {
    /// Represents any type.
    Any = NEOVM_STACK_ITEM_TYPE_ANY,
    /// Represents a code pointer.
    Pointer = NEOVM_STACK_ITEM_TYPE_POINTER,
    /// Represents a boolean value.
    Boolean = NEOVM_STACK_ITEM_TYPE_BOOLEAN,
    /// Represents an integer value.
    Integer = NEOVM_STACK_ITEM_TYPE_INTEGER,
    /// Represents an immutable byte sequence.
    ByteString = NEOVM_STACK_ITEM_TYPE_BYTESTRING,
    /// Represents a mutable byte sequence.
    Buffer = NEOVM_STACK_ITEM_TYPE_BUFFER,
    /// Represents an array.
    Array = NEOVM_STACK_ITEM_TYPE_ARRAY,
    /// Represents a structure.
    Struct = NEOVM_STACK_ITEM_TYPE_STRUCT,
    /// Represents an ordered key-value map.
    Map = NEOVM_STACK_ITEM_TYPE_MAP,
    /// Represents a host interop interface.
    InteropInterface = NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
}

impl StackItemType {
    /// Decodes a C# Neo.VM stack-item type byte.
    #[must_use]
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            NEOVM_STACK_ITEM_TYPE_ANY => Some(Self::Any),
            NEOVM_STACK_ITEM_TYPE_POINTER => Some(Self::Pointer),
            NEOVM_STACK_ITEM_TYPE_BOOLEAN => Some(Self::Boolean),
            NEOVM_STACK_ITEM_TYPE_INTEGER => Some(Self::Integer),
            NEOVM_STACK_ITEM_TYPE_BYTESTRING => Some(Self::ByteString),
            NEOVM_STACK_ITEM_TYPE_BUFFER => Some(Self::Buffer),
            NEOVM_STACK_ITEM_TYPE_ARRAY => Some(Self::Array),
            NEOVM_STACK_ITEM_TYPE_STRUCT => Some(Self::Struct),
            NEOVM_STACK_ITEM_TYPE_MAP => Some(Self::Map),
            NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE => Some(Self::InteropInterface),
            _ => None,
        }
    }

    /// Returns the C# Neo.VM stack-item type byte.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Returns the canonical NeoVM stack-item type name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Pointer => "Pointer",
            Self::Boolean => "Boolean",
            Self::Integer => "Integer",
            Self::ByteString => "ByteString",
            Self::Buffer => "Buffer",
            Self::Array => "Array",
            Self::Struct => "Struct",
            Self::Map => "Map",
            Self::InteropInterface => "InteropInterface",
        }
    }
}

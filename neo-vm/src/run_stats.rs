//! Per-instruction runtime statistics used to price opcodes dynamically.
//!
//! Mirrors `Neo.VM.RunStats` at neo-vm v3.11.2. The jump table fills this in
//! while an instruction executes, and the application engine reads it from the
//! post-instruction hook to compute the opcode price from
//! `HF_Huyao` on (`ApplicationEngine.OpcodeV1`).
//!
//! Opcodes that have no dynamic price calculator ignore these values entirely;
//! they are priced from `ApplicationEngine.StaticCoefficients`.

/// Opcode-specific parameters used to calculate a dynamic price.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunStats {
    /// The `StackItemType` that served as the opcode's operand, when relevant.
    item_type: Option<StackItemType>,

    /// Operand length: the element count of a compound, the byte length of a
    /// buffer or byte string, the number of slots/cells involved, or the number
    /// of stack elements the opcode processed.
    length: i32,

    /// Total change of the reference counter performed by the opcode,
    /// `references_before - references_after`.
    refs_delta: i32,

    /// Number of stack items the opcode cloned.
    cloned_items: i32,
}

/// The stack item type recorded by [`RunStats::collect_type`].
///
/// Only the variants that a dynamic price calculator inspects are modelled;
/// the numeric values match C# `Neo.VM.Types.StackItemType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StackItemType {
    /// `StackItemType.Any`
    Any = 0x00,
    /// `StackItemType.Pointer`
    Pointer = 0x10,
    /// `StackItemType.Boolean`
    Boolean = 0x20,
    /// `StackItemType.Integer`
    Integer = 0x21,
    /// `StackItemType.ByteString`
    ByteString = 0x28,
    /// `StackItemType.Buffer`
    Buffer = 0x30,
    /// `StackItemType.Array`
    Array = 0x40,
    /// `StackItemType.Struct`
    Struct = 0x41,
    /// `StackItemType.Map`
    Map = 0x48,
    /// `StackItemType.InteropInterface`
    InteropInterface = 0x60,
}

impl StackItemType {
    /// Parses a `NEWARRAY_T` type operand (C# casts the byte to `StackItemType`).
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::Any),
            0x10 => Some(Self::Pointer),
            0x20 => Some(Self::Boolean),
            0x21 => Some(Self::Integer),
            0x28 => Some(Self::ByteString),
            0x30 => Some(Self::Buffer),
            0x40 => Some(Self::Array),
            0x41 => Some(Self::Struct),
            0x48 => Some(Self::Map),
            0x60 => Some(Self::InteropInterface),
            _ => None,
        }
    }
}

impl From<crate::stack_item::StackItem> for StackItemType {
    fn from(item: crate::stack_item::StackItem) -> Self {
        Self::from(&item)
    }
}

impl From<&crate::stack_item::StackItem> for StackItemType {
    fn from(item: &crate::stack_item::StackItem) -> Self {
        use crate::stack_item::StackItem;
        match item {
            StackItem::Pointer(_) => Self::Pointer,
            StackItem::Boolean(_) => Self::Boolean,
            StackItem::Integer(_) => Self::Integer,
            StackItem::ByteString(_) => Self::ByteString,
            StackItem::Buffer(_) => Self::Buffer,
            StackItem::Array(_) => Self::Array,
            StackItem::Struct(_) => Self::Struct,
            StackItem::Map(_) => Self::Map,
            StackItem::InteropInterface(_) => Self::InteropInterface,
            StackItem::Null => Self::Any,
        }
    }
}

impl RunStats {
    /// Creates empty statistics.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            item_type: None,
            length: 0,
            refs_delta: 0,
            cloned_items: 0,
        }
    }

    /// Records the `StackItemType` operand of the executed opcode.
    pub fn collect_type(&mut self, item_type: StackItemType) {
        self.item_type = Some(item_type);
    }

    /// Records the length-related statistic, see [`Self::length`].
    pub fn collect_op_length(&mut self, length: i32) {
        self.length = length;
    }

    /// Records the reference counter delta, `before - after`.
    pub fn collect_ref_delta(&mut self, refs_delta: i32) {
        self.refs_delta = refs_delta;
    }

    /// Records the number of items cloned by the opcode.
    pub fn collect_cloned_items(&mut self, cloned_items: i32) {
        self.cloned_items = cloned_items;
    }

    /// The recorded operand type; `StackItemType.Any` when none was recorded.
    #[must_use]
    pub fn item_type(&self) -> StackItemType {
        self.item_type.unwrap_or(StackItemType::Any)
    }

    /// The recorded operand length.
    #[must_use]
    pub const fn length(&self) -> i32 {
        self.length
    }

    /// The recorded reference counter delta.
    #[must_use]
    pub const fn refs_delta(&self) -> i32 {
        self.refs_delta
    }

    /// The recorded number of cloned items.
    #[must_use]
    pub const fn cloned_items(&self) -> i32 {
        self.cloned_items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_unset_values() {
        let stats = RunStats::default();
        assert_eq!(stats.item_type(), StackItemType::Any);
        assert_eq!(stats.length(), 0);
        assert_eq!(stats.refs_delta(), 0);
        assert_eq!(stats.cloned_items(), 0);
    }

    #[test]
    fn collects_each_field() {
        let mut stats = RunStats::new();
        stats.collect_type(StackItemType::Buffer);
        stats.collect_op_length(7);
        stats.collect_ref_delta(-2);
        stats.collect_cloned_items(1);
        assert_eq!(stats.item_type(), StackItemType::Buffer);
        assert_eq!(stats.length(), 7);
        assert_eq!(stats.refs_delta(), -2);
        assert_eq!(stats.cloned_items(), 1);
    }

    #[test]
    fn stack_item_types_match_csharp_discriminants() {
        assert_eq!(StackItemType::Any as u8, 0x00);
        assert_eq!(StackItemType::Pointer as u8, 0x10);
        assert_eq!(StackItemType::Boolean as u8, 0x20);
        assert_eq!(StackItemType::Integer as u8, 0x21);
        assert_eq!(StackItemType::ByteString as u8, 0x28);
        assert_eq!(StackItemType::Buffer as u8, 0x30);
        assert_eq!(StackItemType::Array as u8, 0x40);
        assert_eq!(StackItemType::Struct as u8, 0x41);
        assert_eq!(StackItemType::Map as u8, 0x48);
        assert_eq!(StackItemType::InteropInterface as u8, 0x60);
    }
}

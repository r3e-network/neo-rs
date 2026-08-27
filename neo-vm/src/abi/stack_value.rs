//! Canonical NeoVM stack value types.

use alloc::{format, string::String, vec::Vec};
use core::convert::TryInto;
use serde::{Deserialize, Serialize};

use crate::semantics::numeric;

/// Compact integer tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_INTEGER: u8 = 0;
/// Compact boolean tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_BOOLEAN: u8 = 1;
/// Compact byte string tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_BYTESTRING: u8 = 2;
/// Compact big integer tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_BIG_INTEGER: u8 = 3;
/// Compact array tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_ARRAY: u8 = 4;
/// Compact struct tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_STRUCT: u8 = 5;
/// Compact map tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_MAP: u8 = 6;
/// Compact null tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_NULL: u8 = 7;
/// Compact interop handle tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_INTEROP: u8 = 8;
/// Compact iterator handle tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_ITERATOR: u8 = 9;
/// Compact buffer tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_BUFFER: u8 = 10;
/// Compact pointer tag used by generated RISC-V runtime helpers.
pub const COMPACT_TAG_POINTER: u8 = 11;

/// Shared binary codec tag for integer stack values.
pub const STACK_VALUE_CODEC_TAG_INTEGER: u8 = 0x01;
/// Shared binary codec tag for big integer stack values.
pub const STACK_VALUE_CODEC_TAG_BIG_INTEGER: u8 = 0x02;
/// Shared binary codec tag for byte string stack values.
pub const STACK_VALUE_CODEC_TAG_BYTESTRING: u8 = 0x03;
/// Shared binary codec tag for boolean stack values.
pub const STACK_VALUE_CODEC_TAG_BOOLEAN: u8 = 0x04;
/// Shared binary codec tag for array stack values.
pub const STACK_VALUE_CODEC_TAG_ARRAY: u8 = 0x05;
/// Shared binary codec tag for struct stack values.
pub const STACK_VALUE_CODEC_TAG_STRUCT: u8 = 0x06;
/// Shared binary codec tag for map stack values.
pub const STACK_VALUE_CODEC_TAG_MAP: u8 = 0x07;
/// Shared binary codec tag for interop stack values.
pub const STACK_VALUE_CODEC_TAG_INTEROP: u8 = 0x08;
/// Shared binary codec tag for iterator stack values.
pub const STACK_VALUE_CODEC_TAG_ITERATOR: u8 = 0x09;
/// Shared binary codec tag for null stack values.
pub const STACK_VALUE_CODEC_TAG_NULL: u8 = 0x0A;
/// Shared binary codec tag for pointer stack values.
pub const STACK_VALUE_CODEC_TAG_POINTER: u8 = 0x0B;
/// Shared binary codec tag for buffer stack values.
pub const STACK_VALUE_CODEC_TAG_BUFFER: u8 = 0x0C;

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

/// C# Neo.VM stack item type tags.
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
    /// Converts a C# Neo.VM stack item type byte into a typed tag.
    #[inline]
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

    /// Returns the C# Neo.VM stack item type byte.
    #[inline]
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        match self {
            Self::Any => NEOVM_STACK_ITEM_TYPE_ANY,
            Self::Pointer => NEOVM_STACK_ITEM_TYPE_POINTER,
            Self::Boolean => NEOVM_STACK_ITEM_TYPE_BOOLEAN,
            Self::Integer => NEOVM_STACK_ITEM_TYPE_INTEGER,
            Self::ByteString => NEOVM_STACK_ITEM_TYPE_BYTESTRING,
            Self::Buffer => NEOVM_STACK_ITEM_TYPE_BUFFER,
            Self::Array => NEOVM_STACK_ITEM_TYPE_ARRAY,
            Self::Struct => NEOVM_STACK_ITEM_TYPE_STRUCT,
            Self::Map => NEOVM_STACK_ITEM_TYPE_MAP,
            Self::InteropInterface => NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
        }
    }

    /// Returns the canonical NeoVM stack item type name.
    #[inline]
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

/// Normalize NeoVM StackItemType tags and compact runtime tags into one compact tag space.
#[must_use]
pub fn normalize_stack_item_type_tag(type_tag: u8) -> u8 {
    match type_tag {
        NEOVM_STACK_ITEM_TYPE_BOOLEAN => COMPACT_TAG_BOOLEAN,
        NEOVM_STACK_ITEM_TYPE_INTEGER => COMPACT_TAG_INTEGER,
        NEOVM_STACK_ITEM_TYPE_BYTESTRING => COMPACT_TAG_BYTESTRING,
        NEOVM_STACK_ITEM_TYPE_BUFFER => COMPACT_TAG_BUFFER,
        NEOVM_STACK_ITEM_TYPE_ARRAY => COMPACT_TAG_ARRAY,
        NEOVM_STACK_ITEM_TYPE_STRUCT => COMPACT_TAG_STRUCT,
        NEOVM_STACK_ITEM_TYPE_MAP => COMPACT_TAG_MAP,
        NEOVM_STACK_ITEM_TYPE_POINTER => COMPACT_TAG_POINTER,
        NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE => COMPACT_TAG_INTEROP,
        other => other,
    }
}

/// Return the shared default value for compact runtime tags and non-ambiguous
/// NeoVM StackItemType tags.
///
/// Compact integer is `0`, which is also NeoVM `StackItemType.Any`; use
/// [`new_array_default_value_for_neovm_type_tag`] for `NEWARRAY_T` operands.
#[must_use]
pub fn default_value_for_type_tag(type_tag: u8) -> StackValue {
    match normalize_stack_item_type_tag(type_tag) {
        COMPACT_TAG_BOOLEAN => StackValue::Boolean(false),
        COMPACT_TAG_INTEGER | COMPACT_TAG_BIG_INTEGER => StackValue::Integer(0),
        COMPACT_TAG_BYTESTRING => StackValue::ByteString(Vec::new()),
        COMPACT_TAG_BUFFER => StackValue::Buffer(Vec::new()),
        COMPACT_TAG_ARRAY => StackValue::Array(Vec::new()),
        COMPACT_TAG_STRUCT => StackValue::Struct(Vec::new()),
        COMPACT_TAG_MAP => StackValue::Map(Vec::new()),
        COMPACT_TAG_NULL => StackValue::Null,
        _ => StackValue::Null,
    }
}

/// Return the `NEWARRAY_T` item default for compact runtime tags and
/// non-ambiguous NeoVM StackItemType tags.
///
/// Compact integer is `0`, which is also NeoVM `StackItemType.Any`; use
/// [`new_array_default_value_for_neovm_type_tag`] for raw NeoVM opcode
/// operands.
#[must_use]
pub fn new_array_default_value_for_type_tag(type_tag: u8) -> StackValue {
    match normalize_stack_item_type_tag(type_tag) {
        COMPACT_TAG_INTEGER => StackValue::Integer(0),
        COMPACT_TAG_BYTESTRING => StackValue::ByteString(Vec::new()),
        _ => StackValue::Null,
    }
}

/// Return the NeoVM `NEWARRAY_T` item default for a NeoVM StackItemType operand.
#[must_use]
pub fn new_array_default_value_for_neovm_type_tag(type_tag: u8) -> StackValue {
    match type_tag {
        NEOVM_STACK_ITEM_TYPE_INTEGER => StackValue::Integer(0),
        NEOVM_STACK_ITEM_TYPE_BYTESTRING => StackValue::ByteString(Vec::new()),
        _ => StackValue::Null,
    }
}

/// Pop a syscall-style byte argument from the top of a stack.
///
/// NeoVM byte syscalls accept ByteString and Buffer values as raw bytes. This
/// deliberately does not accept BigInteger, even though it has a byte
/// representation, because syscall argument validation is type based.
pub fn pop_byte_arg(stack: &mut Vec<StackValue>, context: &str) -> Result<Vec<u8>, String> {
    match stack.pop() {
        Some(StackValue::ByteString(bytes)) | Some(StackValue::Buffer(bytes)) => Ok(bytes),
        Some(other) => Err(format!(
            "{context} expects ByteString or Buffer, got {other:?}"
        )),
        None => Err(format!("{context} expects one stack argument")),
    }
}

/// Borrow the content of NeoVM byte sequence values.
///
/// This is intentionally narrower than [`StackValue::as_bytes`]: NeoVM string
/// and byte-array opcodes accept only `ByteString` and `Buffer`, while integer
/// byte representations are valid only for conversion paths.
#[must_use]
pub fn byte_sequence_bytes(value: &StackValue) -> Option<&[u8]> {
    match value {
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => Some(bytes),
        _ => None,
    }
}

/// Return the byte length of a NeoVM `ByteString` or `Buffer`.
#[must_use]
pub fn byte_sequence_len(value: &StackValue) -> Option<usize> {
    byte_sequence_bytes(value).map(<[u8]>::len)
}

/// Extract a strict native-contract boolean result.
///
/// Native contract wrappers should use this helper when decoding host results
/// where only Boolean, Integer, or BigInteger values are acceptable.
#[must_use]
pub fn stack_value_as_bool(value: &StackValue) -> Option<bool> {
    match value {
        StackValue::Boolean(value) => Some(*value),
        StackValue::Integer(_) | StackValue::BigInteger(_) => {
            stack_value_as_i64(value).map(|integer| integer != 0)
        }
        _ => None,
    }
}

/// Extract a strict 64-bit signed integer result.
#[must_use]
pub fn stack_value_as_i64(value: &StackValue) -> Option<i64> {
    match value {
        StackValue::Integer(_) | StackValue::BigInteger(_) => value.to_i128()?.try_into().ok(),
        _ => None,
    }
}

/// Extract a strict 32-bit unsigned integer result.
#[must_use]
pub fn stack_value_as_u32(value: &StackValue) -> Option<u32> {
    stack_value_as_i64(value)?.try_into().ok()
}

/// Extract a strict 8-bit unsigned integer result.
#[must_use]
pub fn stack_value_as_u8(value: &StackValue) -> Option<u8> {
    stack_value_as_i64(value)?.try_into().ok()
}

/// Extract a copy of a NeoVM byte-sequence value.
#[must_use]
pub fn stack_value_as_bytes(value: &StackValue) -> Option<Vec<u8>> {
    byte_sequence_bytes(value).map(<[u8]>::to_vec)
}

/// Return the bytes exposed by NeoVM `StackItem.GetSpan`.
///
/// Splice opcodes such as CAT, LEFT, RIGHT, SUBSTR, and MEMCPY use this path:
/// primitive values expose their memory representation, buffers expose their
/// mutable bytes, and non-span values such as Null or compound references fault.
#[must_use]
pub fn stack_value_span_bytes(value: &StackValue) -> Option<Vec<u8>> {
    match value {
        StackValue::Integer(value) => Some(encode_integer(*value)),
        StackValue::BigInteger(bytes)
        | StackValue::ByteString(bytes)
        | StackValue::Buffer(bytes) => Some(bytes.clone()),
        StackValue::Boolean(value) => Some(alloc::vec![u8::from(*value)]),
        StackValue::Null
        | StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_)
        | StackValue::Pointer(_) => None,
    }
}

/// Extract a fixed-width byte array from a NeoVM byte-sequence value.
#[must_use]
pub fn stack_value_as_fixed_bytes<const N: usize>(value: &StackValue) -> Option<[u8; N]> {
    stack_value_as_bytes(value)?.try_into().ok()
}

/// Extract a UTF-8 string from a NeoVM byte-sequence value.
#[must_use]
pub fn stack_value_as_string(value: &StackValue) -> Option<String> {
    String::from_utf8(stack_value_as_bytes(value)?).ok()
}

/// Extract the owned items from an Array or Struct value.
#[must_use]
pub fn stack_value_into_items(value: StackValue) -> Option<Vec<StackValue>> {
    match value {
        StackValue::Array(items) | StackValue::Struct(items) => Some(items),
        _ => None,
    }
}

/// Concatenate two NeoVM GetSpan-compatible values as the CAT opcode does.
#[must_use]
pub fn concat_splice_values(left: &StackValue, right: &StackValue) -> Option<StackValue> {
    let mut bytes = stack_value_span_bytes(left)?;
    bytes.extend_from_slice(&stack_value_span_bytes(right)?);
    Some(StackValue::Buffer(bytes))
}

/// Slice one NeoVM GetSpan-compatible value as LEFT/RIGHT/SUBSTR do.
#[must_use]
pub fn slice_splice_value(value: &StackValue, index: usize, count: usize) -> Option<StackValue> {
    let bytes = stack_value_span_bytes(value)?;
    let end = index.checked_add(count)?;
    bytes
        .get(index..end)
        .map(|slice| StackValue::Buffer(slice.to_vec()))
}

/// Encode an integer using NeoVM's minimal little-endian two's-complement form.
#[must_use]
pub fn encode_integer(value: i64) -> Vec<u8> {
    if value == 0 {
        return Vec::new();
    }

    let mut bytes = value.to_le_bytes().to_vec();
    if value > 0 {
        while bytes.len() > 1 && bytes.last() == Some(&0) {
            if bytes[bytes.len() - 2] & 0x80 != 0 {
                break;
            }
            bytes.pop();
        }
    } else {
        while bytes.len() > 1 && bytes.last() == Some(&0xff) {
            if bytes[bytes.len() - 2] & 0x80 == 0 {
                break;
            }
            bytes.pop();
        }
    }

    bytes
}

/// NeoVM stack value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StackValue {
    /// 64-bit signed integer for compact ABI paths.
    Integer(i64),
    /// Arbitrary-precision integer encoded as little-endian two's complement.
    BigInteger(Vec<u8>),
    /// Immutable byte string.
    ByteString(Vec<u8>),
    /// Mutable byte buffer.
    Buffer(Vec<u8>),
    /// Boolean value.
    Boolean(bool),
    /// Ordered array.
    Array(Vec<StackValue>),
    /// Ordered struct.
    Struct(Vec<StackValue>),
    /// Key-value map.
    Map(Vec<(StackValue, StackValue)>),
    /// Host interop handle.
    Interop(u64),
    /// Iterator handle.
    Iterator(u64),
    /// Null value.
    Null,
    /// VM pointer.
    Pointer(i64),
}

impl StackValue {
    /// Return the compact runtime tag for this stack value.
    #[must_use]
    pub fn compact_type_tag(&self) -> u8 {
        match self {
            Self::Integer(_) => COMPACT_TAG_INTEGER,
            Self::Boolean(_) => COMPACT_TAG_BOOLEAN,
            Self::ByteString(_) => COMPACT_TAG_BYTESTRING,
            Self::BigInteger(_) => COMPACT_TAG_BIG_INTEGER,
            Self::Array(_) => COMPACT_TAG_ARRAY,
            Self::Struct(_) => COMPACT_TAG_STRUCT,
            Self::Map(_) => COMPACT_TAG_MAP,
            Self::Null => COMPACT_TAG_NULL,
            Self::Interop(_) => COMPACT_TAG_INTEROP,
            Self::Iterator(_) => COMPACT_TAG_ITERATOR,
            Self::Buffer(_) => COMPACT_TAG_BUFFER,
            Self::Pointer(_) => COMPACT_TAG_POINTER,
        }
    }

    /// Convert this value to a NeoVM boolean.
    #[must_use]
    pub fn to_bool(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Boolean(value) => *value,
            Self::Integer(value) => *value != 0,
            Self::BigInteger(bytes) | Self::ByteString(bytes) => {
                bytes.iter().any(|byte| *byte != 0)
            }
            Self::Buffer(_)
            | Self::Array(_)
            | Self::Struct(_)
            | Self::Map(_)
            | Self::Interop(_)
            | Self::Iterator(_)
            | Self::Pointer(_) => true,
        }
    }

    /// Convert this value to a bounded integer when possible.
    ///
    /// Byte values follow NeoVM's little-endian two's complement convention.
    #[must_use]
    pub fn to_i128(&self) -> Option<i128> {
        match self {
            Self::Integer(value) => Some(i128::from(*value)),
            Self::Boolean(value) => Some(i128::from(*value as i8)),
            Self::BigInteger(bytes) | Self::ByteString(bytes) | Self::Buffer(bytes) => {
                numeric::decode_signed_le_bytes_i128(bytes).ok()
            }
            _ => None,
        }
    }

    /// Borrow byte content from byte-like values.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::BigInteger(bytes) | Self::ByteString(bytes) | Self::Buffer(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Convert this value into bytes for NeoVM ByteString-compatible paths.
    ///
    /// This is the shared rule used by runtimes that need byte content without
    /// owning a private StackValue conversion table.
    #[must_use]
    pub fn to_byte_string_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Self::ByteString(bytes) | Self::Buffer(bytes) | Self::BigInteger(bytes) => {
                Some(bytes.clone())
            }
            Self::Integer(value) => Some(encode_integer(*value)),
            Self::Boolean(value) => Some(alloc::vec![u8::from(*value)]),
            Self::Null => Some(Vec::new()),
            Self::Array(_)
            | Self::Struct(_)
            | Self::Map(_)
            | Self::Interop(_)
            | Self::Iterator(_)
            | Self::Pointer(_) => None,
        }
    }

    /// Convert this value as NeoVM `CONVERT` would for a ByteString target.
    #[must_use]
    pub fn convert_to_byte_string_value(&self) -> Option<Self> {
        if matches!(self, Self::Null) {
            return Some(Self::Null);
        }
        self.to_byte_string_bytes().map(Self::ByteString)
    }

    /// Convert this value as NeoVM `CONVERT` would for a Buffer target.
    #[must_use]
    pub fn convert_to_buffer_value(&self) -> Option<Self> {
        if matches!(self, Self::Null) {
            return Some(Self::Null);
        }
        self.to_byte_string_bytes().map(Self::Buffer)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};

    use super::StackValue;

    #[test]
    fn bytes_use_little_endian_twos_complement() {
        assert_eq!(StackValue::ByteString(vec![]).to_i128(), Some(0));
        assert_eq!(StackValue::ByteString(vec![0x7f]).to_i128(), Some(127));
        assert_eq!(StackValue::ByteString(vec![0xff]).to_i128(), Some(-1));
        assert_eq!(
            StackValue::ByteString(vec![0x00, 0x80]).to_i128(),
            Some(-32768)
        );
        assert_eq!(StackValue::ByteString(vec![0xff; 16]).to_i128(), Some(-1));
        assert_eq!(StackValue::ByteString(vec![0x00; 17]).to_i128(), None);
    }

    #[test]
    fn integer_encoding_uses_minimal_little_endian_twos_complement() {
        assert_eq!(super::encode_integer(0), Vec::<u8>::new());
        assert_eq!(super::encode_integer(1), vec![0x01]);
        assert_eq!(super::encode_integer(127), vec![0x7f]);
        assert_eq!(super::encode_integer(128), vec![0x80, 0x00]);
        assert_eq!(super::encode_integer(-1), vec![0xff]);
        assert_eq!(super::encode_integer(-129), vec![0x7f, 0xff]);
    }

    #[test]
    fn stack_values_expose_stable_runtime_type_tags() {
        assert_eq!(
            StackValue::Integer(1).compact_type_tag(),
            super::COMPACT_TAG_INTEGER
        );
        assert_eq!(
            StackValue::Boolean(true).compact_type_tag(),
            super::COMPACT_TAG_BOOLEAN
        );
        assert_eq!(
            StackValue::ByteString(vec![]).compact_type_tag(),
            super::COMPACT_TAG_BYTESTRING
        );
        assert_eq!(
            StackValue::BigInteger(vec![]).compact_type_tag(),
            super::COMPACT_TAG_BIG_INTEGER
        );
        assert_eq!(
            StackValue::Array(vec![]).compact_type_tag(),
            super::COMPACT_TAG_ARRAY
        );
        assert_eq!(
            StackValue::Struct(vec![]).compact_type_tag(),
            super::COMPACT_TAG_STRUCT
        );
        assert_eq!(
            StackValue::Map(vec![]).compact_type_tag(),
            super::COMPACT_TAG_MAP
        );
        assert_eq!(StackValue::Null.compact_type_tag(), super::COMPACT_TAG_NULL);
        assert_eq!(
            StackValue::Interop(1).compact_type_tag(),
            super::COMPACT_TAG_INTEROP
        );
        assert_eq!(
            StackValue::Iterator(1).compact_type_tag(),
            super::COMPACT_TAG_ITERATOR
        );
        assert_eq!(
            StackValue::Buffer(vec![]).compact_type_tag(),
            super::COMPACT_TAG_BUFFER
        );
        assert_eq!(
            StackValue::Pointer(0).compact_type_tag(),
            super::COMPACT_TAG_POINTER
        );
    }

    #[test]
    fn byte_string_conversion_bytes_follow_shared_stack_value_rules() {
        assert_eq!(
            StackValue::ByteString(b"neo".to_vec()).to_byte_string_bytes(),
            Some(b"neo".to_vec())
        );
        assert_eq!(
            StackValue::Buffer(b"n4".to_vec()).to_byte_string_bytes(),
            Some(b"n4".to_vec())
        );
        assert_eq!(
            StackValue::Integer(128).to_byte_string_bytes(),
            Some(vec![0x80, 0x00])
        );
        assert_eq!(
            StackValue::Boolean(true).to_byte_string_bytes(),
            Some(vec![1])
        );
        assert_eq!(
            StackValue::BigInteger(vec![0xff, 0x00]).to_byte_string_bytes(),
            Some(vec![0xff, 0x00])
        );
        assert_eq!(StackValue::Null.to_byte_string_bytes(), Some(vec![]));
        assert_eq!(StackValue::Array(vec![]).to_byte_string_bytes(), None);
    }

    #[test]
    fn conversion_values_follow_neovm_primitive_rules() {
        assert_eq!(
            StackValue::Integer(128).convert_to_byte_string_value(),
            Some(StackValue::ByteString(vec![0x80, 0x00]))
        );
        assert_eq!(
            StackValue::Boolean(false).convert_to_buffer_value(),
            Some(StackValue::Buffer(vec![0]))
        );
        assert_eq!(
            StackValue::BigInteger(vec![0xff, 0x00]).convert_to_buffer_value(),
            Some(StackValue::Buffer(vec![0xff, 0x00]))
        );
        assert_eq!(
            StackValue::Null.convert_to_byte_string_value(),
            Some(StackValue::Null)
        );
        assert_eq!(
            StackValue::Null.convert_to_buffer_value(),
            Some(StackValue::Null)
        );
        assert_eq!(StackValue::Array(vec![]).convert_to_buffer_value(), None);
    }

    #[test]
    fn neovm_type_tags_normalize_to_compact_runtime_tags() {
        assert_eq!(
            super::normalize_stack_item_type_tag(0x20),
            super::COMPACT_TAG_BOOLEAN
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x21),
            super::COMPACT_TAG_INTEGER
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x28),
            super::COMPACT_TAG_BYTESTRING
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x30),
            super::COMPACT_TAG_BUFFER
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x40),
            super::COMPACT_TAG_ARRAY
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x41),
            super::COMPACT_TAG_STRUCT
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(0x48),
            super::COMPACT_TAG_MAP
        );
        assert_eq!(
            super::normalize_stack_item_type_tag(super::COMPACT_TAG_POINTER),
            super::COMPACT_TAG_POINTER
        );
    }

    #[test]
    fn stack_item_type_names_match_neo_vm() {
        assert_eq!(super::StackItemType::Any.name(), "Any");
        assert_eq!(super::StackItemType::Pointer.name(), "Pointer");
        assert_eq!(super::StackItemType::Boolean.name(), "Boolean");
        assert_eq!(super::StackItemType::Integer.name(), "Integer");
        assert_eq!(super::StackItemType::ByteString.name(), "ByteString");
        assert_eq!(super::StackItemType::Buffer.name(), "Buffer");
        assert_eq!(super::StackItemType::Array.name(), "Array");
        assert_eq!(super::StackItemType::Struct.name(), "Struct");
        assert_eq!(super::StackItemType::Map.name(), "Map");
        assert_eq!(
            super::StackItemType::InteropInterface.name(),
            "InteropInterface"
        );
    }

    #[test]
    fn default_values_follow_shared_type_tag_rules() {
        assert_eq!(
            super::default_value_for_type_tag(0x20),
            StackValue::Boolean(false)
        );
        assert_eq!(
            super::default_value_for_type_tag(0x21),
            StackValue::Integer(0)
        );
        assert_eq!(
            super::default_value_for_type_tag(super::COMPACT_TAG_BIG_INTEGER),
            StackValue::Integer(0)
        );
        assert_eq!(
            super::default_value_for_type_tag(0x28),
            StackValue::ByteString(Vec::new())
        );
        assert_eq!(
            super::default_value_for_type_tag(0x30),
            StackValue::Buffer(Vec::new())
        );
        assert_eq!(
            super::default_value_for_type_tag(0x40),
            StackValue::Array(Vec::new())
        );
        assert_eq!(
            super::default_value_for_type_tag(0x41),
            StackValue::Struct(Vec::new())
        );
        assert_eq!(
            super::default_value_for_type_tag(0x48),
            StackValue::Map(Vec::new())
        );
        assert_eq!(
            super::default_value_for_type_tag(super::COMPACT_TAG_NULL),
            StackValue::Null
        );
        assert_eq!(super::default_value_for_type_tag(0xff), StackValue::Null);
    }

    #[test]
    fn new_array_default_values_follow_neovm_rules() {
        assert_eq!(
            super::new_array_default_value_for_type_tag(0x21),
            StackValue::Integer(0)
        );
        assert_eq!(
            super::new_array_default_value_for_type_tag(super::COMPACT_TAG_INTEGER),
            StackValue::Integer(0)
        );
        assert_eq!(
            super::new_array_default_value_for_type_tag(0x28),
            StackValue::ByteString(Vec::new())
        );
        assert_eq!(
            super::new_array_default_value_for_type_tag(super::COMPACT_TAG_BYTESTRING),
            StackValue::ByteString(Vec::new())
        );
        assert_eq!(
            super::new_array_default_value_for_type_tag(0x20),
            StackValue::Null
        );
        assert_eq!(
            super::new_array_default_value_for_type_tag(0xff),
            StackValue::Null
        );
        assert_eq!(
            super::new_array_default_value_for_neovm_type_tag(super::NEOVM_STACK_ITEM_TYPE_ANY),
            StackValue::Null
        );
        assert_eq!(
            super::new_array_default_value_for_neovm_type_tag(super::NEOVM_STACK_ITEM_TYPE_INTEGER),
            StackValue::Integer(0)
        );
        assert_eq!(
            super::new_array_default_value_for_neovm_type_tag(
                super::NEOVM_STACK_ITEM_TYPE_BYTESTRING
            ),
            StackValue::ByteString(Vec::new())
        );
    }

    #[test]
    fn byte_arg_pop_accepts_only_bytestring_and_buffer() {
        let mut byte_string_stack = vec![
            StackValue::Integer(7),
            StackValue::ByteString(b"neo".to_vec()),
        ];
        assert_eq!(
            super::pop_byte_arg(&mut byte_string_stack, "System.Crypto.SHA256"),
            Ok(b"neo".to_vec())
        );
        assert_eq!(byte_string_stack, vec![StackValue::Integer(7)]);

        let mut buffer_stack = vec![StackValue::Buffer(b"n4".to_vec())];
        assert_eq!(
            super::pop_byte_arg(&mut buffer_stack, "System.Crypto.SHA256"),
            Ok(b"n4".to_vec())
        );

        let mut integer_stack = vec![StackValue::Integer(1)];
        let error = super::pop_byte_arg(&mut integer_stack, "System.Crypto.SHA256")
            .expect_err("integer is not a byte syscall argument");
        assert!(error.contains("System.Crypto.SHA256 expects ByteString or Buffer"));

        let mut empty_stack = Vec::new();
        let error = super::pop_byte_arg(&mut empty_stack, "System.Crypto.SHA256")
            .expect_err("empty stack should report missing argument");
        assert_eq!(error, "System.Crypto.SHA256 expects one stack argument");
    }

    #[test]
    fn byte_sequence_helpers_accept_only_bytestring_and_buffer() {
        let byte_string = StackValue::ByteString(b"neo".to_vec());
        let buffer = StackValue::Buffer(b"n4".to_vec());

        assert_eq!(super::byte_sequence_bytes(&byte_string), Some(&b"neo"[..]));
        assert_eq!(super::byte_sequence_bytes(&buffer), Some(&b"n4"[..]));
        assert_eq!(super::byte_sequence_len(&byte_string), Some(3));
        assert_eq!(super::byte_sequence_len(&buffer), Some(2));
        assert_eq!(
            super::byte_sequence_bytes(&StackValue::BigInteger(vec![1])),
            None
        );
        assert_eq!(super::byte_sequence_len(&StackValue::Integer(1)), None);
    }

    #[test]
    fn stack_value_extractors_cover_native_contract_result_shapes() {
        assert_eq!(
            super::stack_value_as_i64(&StackValue::Integer(42)),
            Some(42)
        );
        assert_eq!(
            super::stack_value_as_i64(&StackValue::BigInteger(vec![0xff, 0x00])),
            Some(255)
        );
        assert_eq!(
            super::stack_value_as_i64(&StackValue::BigInteger(vec![0xff; 16])),
            Some(-1)
        );
        assert_eq!(
            super::stack_value_as_i64(&StackValue::BigInteger(vec![
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80,
            ])),
            Some(i64::MIN)
        );
        assert_eq!(
            super::stack_value_as_i64(&StackValue::BigInteger(vec![
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00,
            ])),
            None
        );
        assert_eq!(super::stack_value_as_i64(&StackValue::Null), None);

        assert_eq!(
            super::stack_value_as_bool(&StackValue::Boolean(true)),
            Some(true)
        );
        assert_eq!(
            super::stack_value_as_bool(&StackValue::Integer(0)),
            Some(false)
        );
        assert_eq!(super::stack_value_as_bool(&StackValue::Null), None);

        assert_eq!(
            super::stack_value_as_u32(&StackValue::Integer(4_294_967_295)),
            Some(u32::MAX)
        );
        assert_eq!(super::stack_value_as_u32(&StackValue::Integer(-1)), None);
        assert_eq!(
            super::stack_value_as_u8(&StackValue::Integer(255)),
            Some(255)
        );
        assert_eq!(super::stack_value_as_u8(&StackValue::Integer(256)), None);

        assert_eq!(
            super::stack_value_as_bytes(&StackValue::Buffer(vec![1, 2, 3])),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            super::stack_value_as_fixed_bytes::<4>(&StackValue::ByteString(vec![1, 2, 3, 4])),
            Some([1, 2, 3, 4])
        );
        assert_eq!(
            super::stack_value_as_fixed_bytes::<4>(&StackValue::ByteString(vec![1, 2, 3])),
            None
        );
        assert_eq!(
            super::stack_value_as_string(&StackValue::ByteString(b"neo".to_vec())).as_deref(),
            Some("neo")
        );
        assert_eq!(
            super::stack_value_as_string(&StackValue::ByteString(vec![0xff])),
            None
        );

        assert_eq!(
            super::stack_value_into_items(StackValue::Array(vec![StackValue::Integer(1)])),
            Some(vec![StackValue::Integer(1)])
        );
        assert_eq!(super::stack_value_into_items(StackValue::Integer(1)), None);
    }

    #[test]
    fn concat_splice_values_uses_neovm_span_semantics() {
        assert_eq!(
            super::concat_splice_values(
                &StackValue::ByteString(b"neo".to_vec()),
                &StackValue::Buffer(b"n4".to_vec())
            ),
            Some(StackValue::Buffer(b"neon4".to_vec()))
        );
        assert_eq!(
            super::concat_splice_values(&StackValue::Integer(128), &StackValue::Boolean(true)),
            Some(StackValue::Buffer(vec![0x80, 0x00, 0x01]))
        );
        assert_eq!(
            super::concat_splice_values(&StackValue::Null, &StackValue::Buffer(vec![2])),
            None
        );
    }

    #[test]
    fn slice_splice_value_uses_neovm_span_semantics() {
        assert_eq!(
            super::slice_splice_value(&StackValue::ByteString(b"hello".to_vec()), 1, 3),
            Some(StackValue::Buffer(b"ell".to_vec()))
        );
        assert_eq!(
            super::slice_splice_value(&StackValue::Integer(128), 0, 2),
            Some(StackValue::Buffer(vec![0x80, 0x00]))
        );
        assert_eq!(
            super::slice_splice_value(&StackValue::Buffer(b"hello".to_vec()), 5, 0),
            Some(StackValue::Buffer(Vec::new()))
        );
        assert_eq!(
            super::slice_splice_value(&StackValue::Buffer(b"hello".to_vec()), 4, 2),
            None
        );
        assert_eq!(super::slice_splice_value(&StackValue::Null, 0, 1), None);
    }
}

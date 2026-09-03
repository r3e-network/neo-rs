//! Shared type inspection and conversion semantics for ABI-level NeoVM runtimes.

use alloc::{format, string::String};

use crate::semantics::{arithmetic, comparison, numeric};
use crate::{
    normalize_stack_item_type_tag, StackValue, COMPACT_TAG_ARRAY, COMPACT_TAG_BIG_INTEGER,
    COMPACT_TAG_BOOLEAN, COMPACT_TAG_BUFFER, COMPACT_TAG_BYTESTRING, COMPACT_TAG_INTEGER,
    COMPACT_TAG_STRUCT,
};

/// Return whether a value has the requested compact or NeoVM stack item type.
#[must_use]
pub fn is_type(value: &StackValue, type_tag: u8) -> bool {
    match (
        value.compact_type_tag(),
        normalize_stack_item_type_tag(type_tag),
    ) {
        (COMPACT_TAG_BIG_INTEGER, COMPACT_TAG_INTEGER) => true,
        (actual, expected) => actual == expected,
    }
}

/// Convert a public ABI stack value to the requested compact or NeoVM type.
pub fn convert_value(value: StackValue, target_type: u8) -> Result<StackValue, String> {
    if matches!(value, StackValue::Null) {
        match normalize_stack_item_type_tag(target_type) {
            COMPACT_TAG_INTEGER
            | COMPACT_TAG_BOOLEAN
            | COMPACT_TAG_BYTESTRING
            | COMPACT_TAG_BUFFER
            | COMPACT_TAG_ARRAY
            | COMPACT_TAG_STRUCT => return Ok(StackValue::Null),
            other => return Err(format!("CONVERT: unsupported target type {other}")),
        }
    }

    match normalize_stack_item_type_tag(target_type) {
        COMPACT_TAG_INTEGER => convert_to_integer(&value),
        COMPACT_TAG_BOOLEAN => Ok(StackValue::Boolean(comparison::boolean_value(&value))),
        COMPACT_TAG_BYTESTRING => value
            .convert_to_byte_string_value()
            .ok_or_else(|| "CONVERT: cannot convert to ByteString".into()),
        COMPACT_TAG_BUFFER => value
            .convert_to_buffer_value()
            .ok_or_else(|| "CONVERT: cannot convert to Buffer".into()),
        COMPACT_TAG_ARRAY => match value {
            StackValue::Array(_) => Ok(value),
            StackValue::Struct(items) => Ok(StackValue::Array(items)),
            _ => Err("CONVERT: cannot convert to Array".into()),
        },
        COMPACT_TAG_STRUCT => match value {
            StackValue::Struct(_) => Ok(value),
            StackValue::Array(items) => Ok(StackValue::Struct(items)),
            _ => Err("CONVERT: cannot convert to Struct".into()),
        },
        other => Err(format!("CONVERT: unsupported target type {other}")),
    }
}

fn convert_to_integer(value: &StackValue) -> Result<StackValue, String> {
    match value {
        StackValue::Integer(value) => Ok(StackValue::Integer(*value)),
        StackValue::Boolean(value) => Ok(StackValue::Integer(if *value { 1 } else { 0 })),
        StackValue::ByteString(bytes)
        | StackValue::BigInteger(bytes)
        | StackValue::Buffer(bytes) => arithmetic::numeric_stack_value(
            numeric::decode_signed_le_bytes_bigint(bytes)?,
            "integer size exceeds maximum",
        ),
        StackValue::Null => Ok(StackValue::Null),
        StackValue::Pointer(_)
        | StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_) => Err("CONVERT: cannot convert to Integer".into()),
    }
}

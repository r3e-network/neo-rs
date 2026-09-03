use super::*;
use crate::{
    interpreter::runtime_types::structurally_equal,
    semantics::{
        collections as collection_rules, comparison as comparison_rules,
        conversion as conversion_rules, numeric,
    },
    stack_value_span_bytes, StackItemType, NEOVM_STACK_ITEM_TYPE_ANY, NEOVM_STACK_ITEM_TYPE_ARRAY,
    NEOVM_STACK_ITEM_TYPE_BOOLEAN, NEOVM_STACK_ITEM_TYPE_BUFFER, NEOVM_STACK_ITEM_TYPE_BYTESTRING,
    NEOVM_STACK_ITEM_TYPE_INTEGER, NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE,
    NEOVM_STACK_ITEM_TYPE_MAP, NEOVM_STACK_ITEM_TYPE_STRUCT,
};

#[inline]
pub(crate) fn pop_item(stack: &mut Vec<StackValue>) -> Result<StackValue, String> {
    stack.pop().ok_or_else(|| "stack underflow".to_string())
}

#[inline]
pub(crate) fn pop_integer(stack: &mut Vec<StackValue>) -> Result<i64, String> {
    let value = pop_item(stack)?;
    stack_value_i64(&value, true)?.ok_or_else(|| "expected integer on stack".to_string())
}

pub(crate) fn pop_shift_count(stack: &mut Vec<StackValue>) -> Result<i64, String> {
    let value = pop_item(stack)?;
    if let Some(integer) = stack_value_i64(&value, false)? {
        return Ok(integer);
    }

    match value {
        StackValue::Null | StackValue::Buffer(_, _) => {
            Err("expected integer-compatible value".to_string())
        }
        _ => Err("expected integer-compatible shift count".to_string()),
    }
}

fn stack_value_i64(value: &StackValue, accept_buffer: bool) -> Result<Option<i64>, String> {
    match value {
        StackValue::Integer(value) => Ok(Some(*value)),
        StackValue::Boolean(value) => Ok(Some(if *value { 1 } else { 0 })),
        _ => {
            let bytes = match value {
                StackValue::ByteString(bytes) | StackValue::BigInteger(bytes) => {
                    Some(bytes.as_slice())
                }
                StackValue::Buffer(_, bytes) if accept_buffer => Some(bytes.as_slice()),
                _ => None,
            };
            bytes.map(numeric::decode_signed_le_bytes_i64).transpose()
        }
    }
}

#[inline]
pub(crate) fn integer_value_for_collection_index(value: &StackValue) -> Result<i64, String> {
    collection_rules::collection_index_value(&to_abi_value(value))
}

#[inline]
pub(crate) fn validate_map_key(key: &StackValue) -> Result<(), String> {
    collection_rules::validate_map_key_value(&to_abi_value(key))
}

pub(crate) fn primitive_key_equals(left: &StackValue, right: &StackValue) -> bool {
    collection_rules::primitive_key_equal(&to_abi_value(left), &to_abi_value(right))
}

pub(crate) fn vm_equal(left: &StackValue, right: &StackValue) -> bool {
    match (left, right) {
        (StackValue::Integer(l), StackValue::Integer(r)) => l == r,
        (StackValue::Integer(l), StackValue::BigInteger(r))
        | (StackValue::BigInteger(r), StackValue::Integer(l)) => {
            crate::abi::encode_integer(*l) == *r
        }
        (StackValue::BigInteger(l), StackValue::BigInteger(r)) => l == r,
        (StackValue::ByteString(l), StackValue::ByteString(r)) => l == r,
        (StackValue::Boolean(l), StackValue::Boolean(r)) => l == r,
        (StackValue::Pointer(l), StackValue::Pointer(r)) => l == r,
        (StackValue::Null, StackValue::Null) => true,
        (StackValue::Interop(l), StackValue::Interop(r)) => l == r,
        (StackValue::Iterator(l), StackValue::Iterator(r)) => l == r,
        (StackValue::Array(left_id, _), StackValue::Array(right_id, _))
        | (StackValue::Map(left_id, _), StackValue::Map(right_id, _))
        | (StackValue::Buffer(left_id, _), StackValue::Buffer(right_id, _)) => left_id == right_id,
        (StackValue::Struct(left_id, _), StackValue::Struct(right_id, _))
            if left_id == right_id =>
        {
            true
        }
        (StackValue::Struct(_, _), StackValue::Struct(_, _)) => structurally_equal(left, right),
        _ => false,
    }
}

pub(crate) fn convert_value(
    kind: u8,
    value: StackValue,
    ids: &mut CompoundIds,
) -> Result<StackValue, String> {
    // Validate target type first, even for Null
    match kind {
        NEOVM_STACK_ITEM_TYPE_BOOLEAN
        | NEOVM_STACK_ITEM_TYPE_INTEGER
        | NEOVM_STACK_ITEM_TYPE_BYTESTRING
        | NEOVM_STACK_ITEM_TYPE_BUFFER
        | NEOVM_STACK_ITEM_TYPE_ARRAY
        | NEOVM_STACK_ITEM_TYPE_STRUCT
        | NEOVM_STACK_ITEM_TYPE_MAP
        | NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE => {}
        _ => return Err(format!("unsupported CONVERT target 0x{kind:02x}")),
    }

    if matches!(value, StackValue::Null) {
        return Ok(StackValue::Null);
    }

    match kind {
        NEOVM_STACK_ITEM_TYPE_BOOLEAN
        | NEOVM_STACK_ITEM_TYPE_INTEGER
        | NEOVM_STACK_ITEM_TYPE_BYTESTRING
        | NEOVM_STACK_ITEM_TYPE_BUFFER => {
            let converted = conversion_rules::convert_value(into_abi_value(value), kind)?;
            Ok(ids.import_abi(converted))
        }
        NEOVM_STACK_ITEM_TYPE_ARRAY => Ok(match value {
            StackValue::Array(_, _) => value,
            StackValue::Struct(_, items) => ids.array(items),
            other => return Err(format!("unsupported CONVERT source for Array: {other:?}")),
        }),
        NEOVM_STACK_ITEM_TYPE_STRUCT => Ok(match value {
            StackValue::Struct(_, _) => value,
            StackValue::Array(_, items) => ids.r#struct(items),
            other => return Err(format!("unsupported CONVERT source for Struct: {other:?}")),
        }),
        NEOVM_STACK_ITEM_TYPE_MAP => Ok(match value {
            StackValue::Map(_, _) => value,
            other => return Err(format!("unsupported CONVERT source for Map: {other:?}")),
        }),
        NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE => Ok(match value {
            StackValue::Interop(_) => value,
            other => return Err(format!("unsupported CONVERT source for Interop: {other:?}")),
        }),
        _ => Err(format!("unsupported CONVERT target 0x{kind:02x}")),
    }
}

pub(crate) fn is_type(kind: u8, value: &StackValue) -> Result<bool, String> {
    match StackItemType::from_byte(kind) {
        Some(StackItemType::Any) => Err(format!(
            "unsupported ISTYPE kind {NEOVM_STACK_ITEM_TYPE_ANY:#04x}"
        )),
        Some(_) => Ok(conversion_rules::is_type(&to_abi_value(value), kind)),
        None => Err(format!("unsupported ISTYPE kind 0x{kind:02x}")),
    }
}

#[inline]
pub(crate) fn is_null(value: &StackValue) -> bool {
    comparison_rules::is_null(&to_abi_value(value))
}

#[inline]
pub(crate) fn pop_boolean(stack: &mut Vec<StackValue>) -> Result<bool, String> {
    let value = stack.pop().ok_or_else(|| "stack underflow".to_string())?;
    Ok(comparison_rules::boolean_value(&to_abi_value(&value)))
}

pub(crate) fn pop_bytes(stack: &mut Vec<StackValue>) -> Result<Vec<u8>, String> {
    let value = stack.pop().ok_or_else(|| "stack underflow".to_string())?;
    stack_item_to_bytes(value)
}

/// Convert a StackValue to bytes without consuming it from the stack.
/// Used by CAT to determine result type while extracting byte content.
pub(crate) fn stack_item_to_bytes(item: StackValue) -> Result<Vec<u8>, String> {
    stack_value_span_bytes(&to_abi_value(&item))
        .ok_or_else(|| "expected byte memory-compatible item on stack".to_string())
}

/// Distinguishes short (i8, 1-byte) from long (i32, 4-byte) jump offsets.
pub(crate) enum Offset {
    Short,
    Long,
}

/// Read a jump/call offset from the script at `ip`.
/// Returns (signed offset as isize, byte advance for operand).
pub(crate) fn read_offset(
    script: &[u8],
    ip: usize,
    kind: &Offset,
    name: &str,
) -> Result<(isize, usize), String> {
    match kind {
        Offset::Short => {
            if ip + 2 > script.len() {
                return Err(format!("truncated {name} operand"));
            }
            let offset = i8::from_le_bytes([script[ip + 1]]);
            Ok((offset as isize, 2))
        }
        Offset::Long => {
            if ip + 5 > script.len() {
                return Err(format!("truncated {name}_L operand"));
            }
            let offset = i32::from_le_bytes([
                script[ip + 1],
                script[ip + 2],
                script[ip + 3],
                script[ip + 4],
            ]);
            Ok((offset as isize, 5))
        }
    }
}

/// Compute jump/call target from ip + offset with bounds checking.
pub(crate) fn compute_jump_target_offset(
    ip: usize,
    offset: isize,
    script_len: usize,
    name: &str,
) -> Result<usize, String> {
    let target = ip as isize + offset;
    if target < 0 || target as usize > script_len {
        return Err(format!("{name} target out of bounds"));
    }
    Ok(target as usize)
}

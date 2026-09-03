//! Shared collection semantics for ABI-level NeoVM runtimes.

use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::{
    new_array_default_value_for_neovm_type_tag, semantics::numeric, StackValue, MAX_ITEM_SIZE,
};

/// Convert a primitive NeoVM value into an index used by collection opcodes.
pub fn collection_index_value(value: &StackValue) -> Result<i64, String> {
    match value {
        StackValue::Integer(value) => Ok(*value),
        StackValue::Boolean(value) => Ok(if *value { 1 } else { 0 }),
        StackValue::ByteString(value) | StackValue::BigInteger(value) => {
            numeric::decode_signed_le_bytes_i64(value)
        }
        StackValue::Null => Ok(0),
        _ => Err("expected integer-compatible collection index".into()),
    }
}

/// Validate a NeoVM primitive map key.
pub fn validate_map_key_value(key: &StackValue) -> Result<(), String> {
    match key {
        StackValue::Integer(_) | StackValue::Boolean(_) | StackValue::Null => Ok(()),
        StackValue::ByteString(value) => {
            if value.len() > 64 {
                Err("map key exceeds maximum size".into())
            } else {
                Ok(())
            }
        }
        _ => Err("map key must be primitive".into()),
    }
}

/// Compare primitive map keys by NeoVM map-key equality rules.
#[must_use]
pub fn primitive_key_equal(left: &StackValue, right: &StackValue) -> bool {
    match (left, right) {
        (StackValue::Integer(left), StackValue::Integer(right)) => left == right,
        (StackValue::Boolean(left), StackValue::Boolean(right)) => left == right,
        (StackValue::Null, StackValue::Null) => true,
        (StackValue::ByteString(left), StackValue::ByteString(right)) => left == right,
        _ => false,
    }
}

/// Return the index for a primitive map key.
pub fn map_entry_index(
    pairs: &[(StackValue, StackValue)],
    key: &StackValue,
) -> Result<Option<usize>, String> {
    map_entry_index_by(pairs, key, primitive_key_equal, validate_map_key_value)
}

/// Return the index for a primitive map key using caller-owned value storage.
pub fn map_entry_index_by<T>(
    pairs: &[(T, T)],
    key: &T,
    mut key_equal: impl FnMut(&T, &T) -> bool,
    mut validate_key: impl FnMut(&T) -> Result<(), String>,
) -> Result<Option<usize>, String> {
    validate_key(key)?;
    Ok(pairs
        .iter()
        .position(|(candidate, _)| key_equal(candidate, key)))
}

/// Create a null-filled array.
pub fn new_array(count: i64) -> Result<StackValue, String> {
    let count = non_negative_count(count, "NEWARRAY: negative count")?;
    Ok(StackValue::Array(vec![StackValue::Null; count]))
}

/// Create a type-filled array using NeoVM `NEWARRAY_T` defaults.
pub fn new_array_t(count: i64, type_tag: u8) -> Result<StackValue, String> {
    let count = non_negative_count(count, "NEWARRAY_T: negative count")?;
    Ok(StackValue::Array(vec![
        new_array_default_value_for_neovm_type_tag(type_tag);
        count
    ]))
}

/// Create a null-filled struct.
pub fn new_struct(count: i64) -> Result<StackValue, String> {
    let count = non_negative_count(count, "NEWSTRUCT: negative count")?;
    Ok(StackValue::Struct(vec![StackValue::Null; count]))
}

/// Create a zero-filled buffer.
pub fn new_buffer(size: i64) -> Result<StackValue, String> {
    let size = non_negative_count(size, "NEWBUFFER: negative size")?;
    if size > MAX_ITEM_SIZE {
        return Err("buffer size exceeds MaxItemSize (1MB)".into());
    }
    Ok(StackValue::Buffer(vec![0u8; size]))
}

/// Create an empty ordered map.
#[must_use]
pub fn new_map() -> StackValue {
    StackValue::Map(Vec::new())
}

/// Append a value to an array or struct.
pub fn append(collection: &mut StackValue, value: StackValue) -> Result<(), String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            items.push(value);
            Ok(())
        }
        _ => Err("APPEND: top-1 is not an array or struct".into()),
    }
}

/// Set a collection item.
pub fn set_item(
    collection: &mut StackValue,
    key: StackValue,
    value: StackValue,
) -> Result<(), String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            let idx = array_index(
                &key,
                "SETITEM: non-integer index for array/struct",
                "SETITEM: index out of range",
            )?;
            if idx >= items.len() {
                return Err("SETITEM: index out of range".into());
            }
            items[idx] = value;
            Ok(())
        }
        StackValue::Map(pairs) => {
            if let Some(index) = map_entry_index(pairs, &key)? {
                pairs[index].1 = value;
                return Ok(());
            }
            pairs.push((key, value));
            Ok(())
        }
        StackValue::Buffer(bytes) => {
            let idx = buffer_index(&key, "SETITEM: buffer requires integer key and value")?;
            let value = match value {
                StackValue::Integer(value) => value,
                _ => return Err("SETITEM: buffer requires integer key and value".into()),
            };
            if idx >= bytes.len() {
                return Err("SETITEM: buffer index out of range".into());
            }
            #[allow(clippy::cast_sign_loss)]
            {
                bytes[idx] = value as u8;
            }
            Ok(())
        }
        _ => Err("SETITEM: not a collection".into()),
    }
}

/// Pick an item from a collection-like value.
pub fn pick_item(collection: &StackValue, key: &StackValue) -> Result<StackValue, String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            let Some(index) = non_negative_index(collection_index_value(key)?) else {
                return Err("PICKITEM: index out of range".into());
            };
            items
                .get(index)
                .cloned()
                .ok_or_else(|| "PICKITEM: index out of range".into())
        }
        StackValue::Map(pairs) => {
            let Some(index) = map_entry_index(pairs, key)? else {
                return Err("PICKITEM: key not found in map".into());
            };
            Ok(pairs[index].1.clone())
        }
        StackValue::ByteString(bytes) => pick_byte(bytes, key, "PICKITEM: byte index out of range"),
        StackValue::Buffer(bytes) => pick_byte(bytes, key, "PICKITEM: buffer index out of range"),
        StackValue::Integer(_) | StackValue::BigInteger(_) | StackValue::Boolean(_) => {
            let bytes = primitive_memory(collection);
            pick_byte(&bytes, key, "PICKITEM: byte index out of range")
        }
        _ => Err("PICKITEM: unsupported types".into()),
    }
}

/// Remove a key from a mutable collection.
pub fn remove(collection: &mut StackValue, key: &StackValue) -> Result<(), String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            let idx = array_index(
                key,
                "REMOVE: non-integer index for array/struct",
                "REMOVE: index out of range",
            )?;
            if idx >= items.len() {
                return Err("REMOVE: index out of range".into());
            }
            items.remove(idx);
            Ok(())
        }
        StackValue::Map(pairs) => {
            if let Some(index) = map_entry_index(pairs, key)? {
                pairs.remove(index);
            }
            Ok(())
        }
        _ => Err("REMOVE: not a collection".into()),
    }
}

/// Return collection/string/buffer size.
pub fn size(value: &StackValue) -> Result<i64, String> {
    match value {
        StackValue::Array(items) | StackValue::Struct(items) => Ok(items.len() as i64),
        StackValue::Map(pairs) => Ok(pairs.len() as i64),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => Ok(bytes.len() as i64),
        StackValue::Integer(_) | StackValue::BigInteger(_) | StackValue::Boolean(_) => {
            Ok(primitive_memory(value).len() as i64)
        }
        _ => Err("SIZE: unsupported type".into()),
    }
}

/// Return whether a key exists in a collection-like value.
pub fn has_key(collection: &StackValue, key: &StackValue) -> Result<bool, String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            Ok(non_negative_index(collection_index_value(key)?)
                .is_some_and(|index| index < items.len()))
        }
        StackValue::Map(pairs) => Ok(map_entry_index(pairs, key)?.is_some()),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
            Ok(non_negative_index(collection_index_value(key)?)
                .is_some_and(|index| index < bytes.len()))
        }
        _ => Err("HASKEY: unsupported types".into()),
    }
}

/// Return map keys as an array.
pub fn keys(value: StackValue) -> Result<StackValue, String> {
    match value {
        StackValue::Map(pairs) => Ok(StackValue::Array(
            pairs.into_iter().map(|(key, _)| key).collect(),
        )),
        _ => Err("KEYS: not a map".into()),
    }
}

/// Return map values or array/struct values as an array.
pub fn values(value: StackValue) -> Result<StackValue, String> {
    match value {
        StackValue::Map(pairs) => Ok(StackValue::Array(
            pairs.into_iter().map(|(_, value)| value).collect(),
        )),
        StackValue::Array(items) | StackValue::Struct(items) => Ok(StackValue::Array(items)),
        _ => Err("VALUES: not a map or array".into()),
    }
}

/// Pack already ordered values as an array.
#[must_use]
pub fn pack(items: Vec<StackValue>) -> StackValue {
    StackValue::Array(items)
}

/// Unpack array/struct values followed by their count.
pub fn unpack(value: StackValue) -> Result<Vec<StackValue>, String> {
    match value {
        StackValue::Array(mut items) | StackValue::Struct(mut items) => {
            let count = items.len() as i64;
            items.push(StackValue::Integer(count));
            Ok(items)
        }
        _ => Err("UNPACK: not an array or struct".into()),
    }
}

/// Reverse an array or struct in place.
pub fn reverse_items(collection: &mut StackValue) -> Result<(), String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            items.reverse();
            Ok(())
        }
        _ => Err("REVERSEITEMS: not an array or struct".into()),
    }
}

/// Clear array, struct, or map items.
pub fn clear_items(collection: &mut StackValue) -> Result<(), String> {
    match collection {
        StackValue::Array(items) | StackValue::Struct(items) => {
            items.clear();
            Ok(())
        }
        StackValue::Map(pairs) => {
            pairs.clear();
            Ok(())
        }
        _ => Err("CLEARITEMS: not a collection".into()),
    }
}

/// Pop one item from a collection-like value, returning values to push in order.
pub fn pop_item(value: StackValue) -> Result<Vec<StackValue>, String> {
    match value {
        StackValue::Array(mut items) => items
            .pop()
            .map(|value| vec![value])
            .ok_or_else(|| "POPITEM: array is empty".into()),
        StackValue::Struct(mut items) => items
            .pop()
            .map(|value| vec![value])
            .ok_or_else(|| "POPITEM: struct is empty".into()),
        StackValue::Map(mut pairs) => pairs
            .pop()
            .map(|(key, value)| vec![key, value])
            .ok_or_else(|| "POPITEM: map is empty".into()),
        StackValue::Buffer(mut bytes) => bytes
            .pop()
            .map(|value| vec![StackValue::Integer(i64::from(value))])
            .ok_or_else(|| "POPITEM: buffer is empty".into()),
        _ => Err("POPITEM: not a collection".into()),
    }
}

/// Pack values as a struct.
#[must_use]
pub fn pack_struct(items: Vec<StackValue>) -> StackValue {
    StackValue::Struct(items)
}

/// Pack key/value pairs as a map.
#[must_use]
pub fn pack_map(pairs: Vec<(StackValue, StackValue)>) -> StackValue {
    StackValue::Map(pairs)
}

pub(crate) fn non_negative_count(value: i64, error: &'static str) -> Result<usize, String> {
    usize::try_from(value).map_err(|_| error.into())
}

fn non_negative_index(value: i64) -> Option<usize> {
    usize::try_from(value).ok()
}

fn array_index(
    key: &StackValue,
    type_error: &'static str,
    range_error: &'static str,
) -> Result<usize, String> {
    let index = collection_index_value(key).map_err(|_| type_error.to_string())?;
    non_negative_index(index).ok_or_else(|| range_error.into())
}

fn buffer_index(key: &StackValue, type_error: &'static str) -> Result<usize, String> {
    let index = collection_index_value(key).map_err(|_| type_error.to_string())?;
    non_negative_index(index).ok_or_else(|| "SETITEM: buffer index out of range".into())
}

fn pick_byte(
    bytes: &[u8],
    key: &StackValue,
    range_error: &'static str,
) -> Result<StackValue, String> {
    let Some(index) = non_negative_index(collection_index_value(key)?) else {
        return Err(range_error.into());
    };
    bytes
        .get(index)
        .map(|value| StackValue::Integer(i64::from(*value)))
        .ok_or_else(|| range_error.into())
}

fn primitive_memory(value: &StackValue) -> Vec<u8> {
    match value {
        StackValue::Integer(_) | StackValue::BigInteger(_) | StackValue::Boolean(_) => value
            .to_byte_string_bytes()
            .expect("primitive values have byte-string memory"),
        _ => Vec::new(),
    }
}

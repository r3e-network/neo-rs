#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

use neo_vm::stack_item::array::Array as ArrayItem;
use neo_vm::stack_item::map::Map as MapItem;
use neo_vm::stack_item::struct_item::Struct as StructItem;
use neo_vm::{StackItem, StackItemType};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::{BTreeMap, HashSet};

/// JSON serialization utilities for VM stack items.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Maximum safe integer representable without loss in JSON.
    pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;
    pub const MIN_SAFE_INTEGER: i64 = -9007199254740991;

    /// Serializes a [`StackItem`] to a UTF-8 JSON byte vector.
    pub fn serialize_to_byte_array(item: &StackItem, max_size: u32) -> Result<Vec<u8>, String> {
        let json = Self::serialize_to_json(item)?;
        let payload = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
        if payload.len() > max_size as usize {
            return Err("JSON output too large".to_string());
        }
        Ok(payload)
    }

    /// Serializes a stack item to a [`JsonValue`].
    pub fn serialize_to_json(item: &StackItem) -> Result<JsonValue, String> {
        let mut seen = HashSet::new();
        Self::serialize_internal(item, &mut seen)
    }

    fn serialize_internal(
        item: &StackItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        match item {
            StackItem::Null => Ok(JsonValue::Null),
            StackItem::Boolean(value) => Ok(JsonValue::Bool(*value)),
            StackItem::Integer(integer) => {
                let int_value = integer
                    .to_i64()
                    .ok_or_else(|| "Integer too large for JSON".to_string())?;
                if !(Self::MIN_SAFE_INTEGER..=Self::MAX_SAFE_INTEGER).contains(&int_value) {
                    return Err("Integer out of safe JSON range".to_string());
                }
                Ok(JsonValue::Number(JsonNumber::from(int_value)))
            }
            StackItem::ByteString(bytes) => {
                let string = String::from_utf8_lossy(bytes).to_string();
                Ok(JsonValue::String(string))
            }
            StackItem::Buffer(buffer) => {
                let string = String::from_utf8_lossy(buffer.data()).to_string();
                Ok(JsonValue::String(string))
            }
            StackItem::Array(array) => Self::serialize_compound_array(array, seen),
            StackItem::Struct(struct_item) => Self::serialize_compound_struct(struct_item, seen),
            StackItem::Map(map) => Self::serialize_map(map, seen),
            StackItem::Pointer(_) => Err("Cannot serialize Pointer to JSON".to_string()),
            StackItem::InteropInterface(_) => {
                Err("Cannot serialize InteropInterface to JSON".to_string())
            }
        }
    }

    fn serialize_compound_array(
        array: &ArrayItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        let identity = (array.id(), StackItemType::Array);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
        }

        let mut result = Vec::with_capacity(array.len());
        for element in array.items() {
            result.push(Self::serialize_internal(element, seen)?);
        }

        seen.remove(&identity);
        Ok(JsonValue::Array(result))
    }

    fn serialize_compound_struct(
        structure: &StructItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        let identity = (structure.id(), StackItemType::Struct);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
        }

        let mut result = Vec::with_capacity(structure.len());
        for element in structure.items() {
            result.push(Self::serialize_internal(element, seen)?);
        }

        seen.remove(&identity);
        Ok(JsonValue::Array(result))
    }

    fn serialize_map(
        map: &MapItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        let identity = (map.id(), StackItemType::Map);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
        }

        let mut result = JsonMap::new();
        for (key, value) in map.items() {
            let key_bytes = match key {
                StackItem::ByteString(bytes) => bytes.clone(),
                StackItem::Buffer(buffer) => buffer.data().to_vec(),
                _ => return Err("Map key must be a byte string".to_string()),
            };

            let key_str =
                String::from_utf8(key_bytes).map_err(|_| "Invalid UTF-8 in map key".to_string())?;

            let value_json = Self::serialize_internal(value, seen)?;
            result.insert(key_str, value_json);
        }

        seen.remove(&identity);
        Ok(JsonValue::Object(result))
    }

    /// Deserializes a JSON payload into a [`StackItem`].
    pub fn deserialize(json: &[u8], max_depth: usize) -> Result<StackItem, String> {
        let value: JsonValue = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        Self::deserialize_from_json(&value, max_depth)
    }

    /// Deserializes a [`JsonValue`] into a stack item.
    pub fn deserialize_from_json(value: &JsonValue, max_depth: usize) -> Result<StackItem, String> {
        Self::deserialize_internal(value, 0, max_depth)
    }

    fn deserialize_internal(
        value: &JsonValue,
        depth: usize,
        max_depth: usize,
    ) -> Result<StackItem, String> {
        if depth >= max_depth {
            return Err("Maximum JSON depth exceeded".to_string());
        }

        match value {
            JsonValue::Null => Ok(StackItem::null()),
            JsonValue::Bool(b) => Ok(StackItem::from_bool(*b)),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(StackItem::from_int(BigInt::from(i)))
                } else if let Some(u) = n.as_u64() {
                    Ok(StackItem::from_int(BigInt::from(u)))
                } else {
                    Err("Unsupported JSON number".to_string())
                }
            }
            JsonValue::String(s) => Ok(StackItem::from_byte_string(s.as_bytes())),
            JsonValue::Array(arr) => {
                let mut items = Vec::with_capacity(arr.len());
                for element in arr {
                    items.push(Self::deserialize_internal(element, depth + 1, max_depth)?);
                }
                Ok(StackItem::from_array(items))
            }
            JsonValue::Object(obj) => {
                let mut map = BTreeMap::new();
                for (key, element) in obj {
                    let key_item = StackItem::from_byte_string(key.as_bytes());
                    let value_item = Self::deserialize_internal(element, depth + 1, max_depth)?;
                    map.insert(key_item, value_item);
                }
                Ok(StackItem::Map(MapItem::new_untracked(map)))
            }
        }
    }
}

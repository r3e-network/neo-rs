#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

use crate::neo_vm::stack_item::array::Array as ArrayItem;
use crate::neo_vm::stack_item::map::Map as MapItem;
use crate::neo_vm::stack_item::struct_item::Struct as StructItem;
use crate::neo_vm::{StackItem, StackItemType};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::HashSet;

/// JSON serialization utilities for VM stack items.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Maximum safe integer representable without loss in JSON.
    pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;
    pub const MIN_SAFE_INTEGER: i64 = -9007199254740991;

    /// Serializes a [`StackItem`] to a UTF-8 JSON byte vector.
    ///
    /// String escape behavior matches C# Neo's `StdLib.jsonSerialize`, which uses
    /// .NET's `System.Text.Json.JsonSerializer` with default `JavaScriptEncoder.Default`.
    /// The default encoder escapes `"`, `\`, control chars, all non-ASCII, plus
    /// `<`, `>`, `&`, `'`, `+`, `` ` ``. This differs from serde_json's minimal escape
    /// set, so we bypass serde_json's string output and write our own JSON encoder.
    pub fn serialize_to_byte_array(item: &StackItem, max_size: u32) -> Result<Vec<u8>, String> {
        let json = Self::serialize_to_json(item)?;
        let mut payload = Vec::new();
        Self::write_json_value(&json, &mut payload);
        if payload.len() > max_size as usize {
            return Err("JSON output too large".to_string());
        }
        Ok(payload)
    }

    /// Encodes a `serde_json::Value` to UTF-8 JSON bytes using .NET
    /// `System.Text.Json.JsonSerializer` default semantics — i.e. with
    /// `JavaScriptEncoder.Default` escaping. Use this anywhere a contract
    /// manifest, native ABI member, or other persisted JSON payload must
    /// be byte-identical with C# Neo v3.x.
    pub fn encode_value_csharp_compatible(value: &JsonValue) -> Vec<u8> {
        let mut out = Vec::new();
        Self::write_json_value(value, &mut out);
        out
    }

    /// Writes a [`JsonValue`] using C#-compatible escape semantics.
    fn write_json_value(value: &JsonValue, out: &mut Vec<u8>) {
        match value {
            JsonValue::Null => out.extend_from_slice(b"null"),
            JsonValue::Bool(true) => out.extend_from_slice(b"true"),
            JsonValue::Bool(false) => out.extend_from_slice(b"false"),
            JsonValue::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
            JsonValue::String(s) => Self::write_json_string(s, out),
            JsonValue::Array(items) => {
                out.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    Self::write_json_value(item, out);
                }
                out.push(b']');
            }
            JsonValue::Object(obj) => {
                out.push(b'{');
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    Self::write_json_string(k, out);
                    out.push(b':');
                    Self::write_json_value(v, out);
                }
                out.push(b'}');
            }
        }
    }

    /// Writes a JSON string with .NET `JavaScriptEncoder.Default`-compatible escaping.
    /// Escapes: `"`, `\`, control chars (<0x20), DEL (0x7F), all non-ASCII (≥0x80),
    /// plus `<` `>` `&` `'` `+` `` ` `` (the JS-safe additional set).
    fn write_json_string(s: &str, out: &mut Vec<u8>) {
        out.push(b'"');
        for c in s.chars() {
            match c {
                '"' => out.extend_from_slice(b"\\\""),
                '\\' => out.extend_from_slice(b"\\\\"),
                '\u{0008}' => out.extend_from_slice(b"\\b"),
                '\u{000C}' => out.extend_from_slice(b"\\f"),
                '\n' => out.extend_from_slice(b"\\n"),
                '\r' => out.extend_from_slice(b"\\r"),
                '\t' => out.extend_from_slice(b"\\t"),
                c if (c as u32) < 0x20
                    || (c as u32) == 0x7F
                    || (c as u32) > 0x7F
                    || matches!(c, '<' | '>' | '&' | '\'' | '+' | '`') =>
                {
                    let cp = c as u32;
                    if cp <= 0xFFFF {
                        let _ = std::io::Write::write_fmt(out, format_args!("\\u{:04X}", cp));
                    } else {
                        // Encode supplementary chars as UTF-16 surrogate pair.
                        let v = cp - 0x10000;
                        let hi = 0xD800 + (v >> 10);
                        let lo = 0xDC00 + (v & 0x3FF);
                        let _ = std::io::Write::write_fmt(
                            out,
                            format_args!("\\u{:04X}\\u{:04X}", hi, lo),
                        );
                    }
                }
                c => {
                    // ASCII-safe printable char.
                    out.push(c as u8);
                }
            }
        }
        out.push(b'"');
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
                let data = buffer.data();
                let string = String::from_utf8_lossy(&data).to_string();
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
            result.push(Self::serialize_internal(&element, seen)?);
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
            result.push(Self::serialize_internal(&element, seen)?);
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
                StackItem::Buffer(buffer) => buffer.data(),
                _ => return Err("Map key must be a byte string".to_string()),
            };

            let key_str =
                String::from_utf8(key_bytes).map_err(|_| "Invalid UTF-8 in map key".to_string())?;

            let value_json = Self::serialize_internal(&value, seen)?;
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
                // C# parity: JSON object → StackItem::Map preserves insertion order.
                // serde_json's `preserve_order` feature is enabled (workspace Cargo.toml),
                // so iterating `obj` already yields entries in source order. Construct via
                // Vec<(K, V)> instead of BTreeMap (which would alphabetically sort the keys).
                let mut entries = Vec::with_capacity(obj.len());
                for (key, element) in obj {
                    let key_item = StackItem::from_byte_string(key.as_bytes());
                    let value_item = Self::deserialize_internal(element, depth + 1, max_depth)?;
                    entries.push((key_item, value_item));
                }
                Ok(StackItem::Map(MapItem::new_untracked(entries)))
            }
        }
    }
}

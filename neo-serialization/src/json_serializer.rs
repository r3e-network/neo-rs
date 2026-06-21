#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

use neo_error::{CoreError, CoreResult};
use neo_vm::StackItem;
use neo_vm::stack_item::{Array, Map as MapItem, Struct};
use neo_vm_rs::StackItemType;
use num_bigint::BigInt;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::HashSet;

/// JSON serialization utilities for VM stack items.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Maximum safe integer representable without loss in JSON.
    pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;
    /// Minimum safe integer representable without loss in JSON.
    pub const MIN_SAFE_INTEGER: i64 = -9007199254740991;

    /// Serializes a [`StackItem`] to a UTF-8 JSON byte vector, matching C# Neo's
    /// `StdLib.jsonSerialize` (`JsonSerializer.SerializeToByteArray`).
    ///
    /// String escaping matches `System.Text.Json`'s default
    /// `JavaScriptEncoder.Default` (see [`encode_value_csharp_compatible`]).
    /// `max_size` mirrors C#'s `engine.Limits.MaxItemSize` bound: a payload longer
    /// than the limit faults (C# throws `InvalidOperationException`).
    ///
    /// [`encode_value_csharp_compatible`]: Self::encode_value_csharp_compatible
    pub fn serialize_to_byte_array(item: &StackItem, max_size: u32) -> CoreResult<Vec<u8>> {
        let json = Self::serialize_to_json(item)?;
        let payload = Self::encode_value_csharp_compatible(&json);
        if payload.len() > max_size as usize {
            return Err(CoreError::other("JSON output too large"));
        }
        Ok(payload)
    }

    /// Encodes a `serde_json::Value` to UTF-8 JSON bytes byte-identically with
    /// C# Neo's `System.Text.Json` default output (`JavaScriptEncoder.Default`).
    ///
    /// Delegates to [`crate::json::escape`], whose `CSharpEscapeFormatter` reproduces
    /// the default encoder exactly: the quote is emitted as `"` (not `\"`),
    /// `\`/`\b`/`\f`/`\n`/`\r`/`\t` use their short forms, `/` is left unescaped,
    /// and `<` `>` `&` `'` `+` `` ` `` plus every non-ASCII code point are emitted
    /// as `\uXXXX` (uppercase hex, surrogate pairs for astral-plane chars).
    /// Use this anywhere a manifest, native ABI member, or other persisted JSON
    /// payload must match C# Neo v3.x.
    pub fn encode_value_csharp_compatible(value: &JsonValue) -> Vec<u8> {
        // serde_json::Value serialization writes only to the in-memory buffer and
        // cannot fail, so the (impossible) error is surfaced as a panic guarding
        // the invariant rather than silently corrupting output.
        crate::json::escape::to_vec(value, false)
            .expect("serde_json::Value serializes to an in-memory buffer infallibly")
    }

    /// Serializes a stack item to a [`JsonValue`].
    pub fn serialize_to_json(item: &StackItem) -> CoreResult<JsonValue> {
        let mut seen = HashSet::new();
        Self::serialize_internal(item, &mut seen)
    }

    fn serialize_internal(
        item: &StackItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> CoreResult<JsonValue> {
        match item {
            StackItem::Null => Ok(JsonValue::Null),
            StackItem::Boolean(value) => Ok(JsonValue::Bool(*value)),
            StackItem::Integer(integer) => {
                let int_value = integer
                    .to_i64()
                    .filter(|v| (Self::MIN_SAFE_INTEGER..=Self::MAX_SAFE_INTEGER).contains(v))
                    .ok_or_else(|| CoreError::other("Integer out of safe JSON range"))?;
                Ok(JsonValue::Number(JsonNumber::from(int_value)))
            }
            StackItem::ByteString(bytes) => {
                let string = String::from_utf8_lossy(bytes).to_string();
                Ok(JsonValue::String(string))
            }
            StackItem::Buffer(buffer) => {
                let string = String::from_utf8_lossy(&buffer.data()).to_string();
                Ok(JsonValue::String(string))
            }
            StackItem::Array(array) => Self::serialize_compound_array(array, seen),
            StackItem::Struct(struct_item) => Self::serialize_compound_struct(struct_item, seen),
            StackItem::Map(map) => Self::serialize_map(map, seen),
            StackItem::Pointer(_) => Err(CoreError::other("Cannot serialize Pointer to JSON")),
            StackItem::InteropInterface(_) => Err(CoreError::other(
                "Cannot serialize InteropInterface to JSON",
            )),
        }
    }

    fn serialize_compound_array(
        array: &Array,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> CoreResult<JsonValue> {
        let identity = (array.id(), StackItemType::Array);
        if !seen.insert(identity) {
            return Err(CoreError::other("Circular reference detected"));
        }

        let mut result = Vec::with_capacity(array.len());
        for element in array.iter() {
            result.push(Self::serialize_internal(&element, seen)?);
        }

        seen.remove(&identity);
        Ok(JsonValue::Array(result))
    }

    fn serialize_compound_struct(
        structure: &Struct,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> CoreResult<JsonValue> {
        let identity = (structure.id(), StackItemType::Struct);
        if !seen.insert(identity) {
            return Err(CoreError::other("Circular reference detected"));
        }

        let mut result = Vec::with_capacity(structure.len());
        for element in structure.iter() {
            result.push(Self::serialize_internal(&element, seen)?);
        }

        seen.remove(&identity);
        Ok(JsonValue::Array(result))
    }

    fn serialize_map(
        map: &MapItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> CoreResult<JsonValue> {
        let identity = (map.id(), StackItemType::Map);
        if !seen.insert(identity) {
            return Err(CoreError::other("Circular reference detected"));
        }

        let mut result = JsonMap::new();
        for (key, value) in map.iter() {
            let key_bytes = match &key {
                StackItem::ByteString(bytes) => bytes.clone(),
                StackItem::Buffer(buffer) => buffer.data(),
                _ => return Err(CoreError::other("Map key must be a byte string")),
            };

            let key_str = String::from_utf8(key_bytes)
                .map_err(|_| CoreError::other("Invalid UTF-8 in map key"))?;

            let value_json = Self::serialize_internal(&value, seen)?;
            result.insert(key_str, value_json);
        }

        seen.remove(&identity);
        Ok(JsonValue::Object(result))
    }

    /// Deserializes a JSON payload into a [`StackItem`].
    ///
    /// `max_depth` bounds nesting (C# `JToken.Parse(json, 10)`); `max_items`
    /// bounds the total produced item count (C# `JsonSerializer.Deserialize`
    /// decrements `engine.Limits.MaxStackSize`, default 2048, once per item and
    /// once per map entry, faulting when exhausted). Both faults match C#.
    pub fn deserialize(json: &[u8], max_depth: usize, max_items: usize) -> CoreResult<StackItem> {
        let value: JsonValue =
            serde_json::from_slice(json).map_err(|e| CoreError::other(e.to_string()))?;
        Self::deserialize_from_json(&value, max_depth, max_items)
    }

    /// Deserializes a [`JsonValue`] into a stack item (see [`deserialize`] for the
    /// limit semantics).
    ///
    /// [`deserialize`]: Self::deserialize
    pub fn deserialize_from_json(
        value: &JsonValue,
        max_depth: usize,
        max_items: usize,
    ) -> CoreResult<StackItem> {
        let mut remaining = max_items;
        Self::deserialize_internal(value, 0, max_depth, &mut remaining)
    }

    fn deserialize_internal(
        value: &JsonValue,
        depth: usize,
        max_depth: usize,
        remaining: &mut usize,
    ) -> CoreResult<StackItem> {
        if depth >= max_depth {
            return Err(CoreError::other("Maximum JSON depth exceeded"));
        }
        // C# decrements maxStackSize before processing each item and faults when
        // it was already 0 (`if (maxStackSize-- == 0) throw`).
        if *remaining == 0 {
            return Err(CoreError::other("Max stack size reached"));
        }
        *remaining -= 1;

        match value {
            JsonValue::Null => Ok(StackItem::null()),
            JsonValue::Bool(b) => Ok(StackItem::from_bool(*b)),
            JsonValue::Number(n) => {
                // C# JsonSerializer.Deserialize treats every JNumber as a double:
                // a fractional value faults ("Decimal value is not allowed"), and
                // an integer-valued number becomes a BigInteger (post-Basilisk via
                // BigInteger.Parse(value.ToString())). serde_json types integer
                // literals as i64/u64 and only non-integers as f64, so check those
                // first — they are exact — and fall back to the double path.
                if let Some(i) = n.as_i64() {
                    Ok(StackItem::from_int(BigInt::from(i)))
                } else if let Some(u) = n.as_u64() {
                    Ok(StackItem::from_int(BigInt::from(u)))
                } else {
                    let f = n
                        .as_f64()
                        .ok_or_else(|| CoreError::other("Invalid JSON number"))?;
                    if f.fract() != 0.0 {
                        return Err(CoreError::other("Decimal value is not allowed"));
                    }
                    // `{f}` is the shortest round-trippable decimal in fixed (never
                    // scientific) notation — matching the integer value C# obtains
                    // from `BigInteger.Parse(double.ToString())`, not the double's
                    // exact stored value (which `{:.0}` would give).
                    let big = format!("{f}")
                        .parse::<BigInt>()
                        .map_err(|_| CoreError::other("Invalid JSON integer"))?;
                    Ok(StackItem::from_int(big))
                }
            }
            JsonValue::String(s) => Ok(StackItem::from_byte_string(s.as_bytes())),
            JsonValue::Array(arr) => {
                let mut items = Vec::with_capacity(arr.len());
                for element in arr {
                    items.push(Self::deserialize_internal(
                        element,
                        depth + 1,
                        max_depth,
                        remaining,
                    )?);
                }
                Ok(StackItem::from_array(items))
            }
            JsonValue::Object(obj) => {
                // C# parity: JSON object → StackItem::Map preserves insertion order.
                // serde_json's `preserve_order` feature is enabled (workspace Cargo.toml),
                // so iterating `obj` already yields entries in source order. Build a
                // Vec<(K, V)> (not a BTreeMap, which would alphabetically sort the keys)
                // and hand it straight to the ordered map item.
                let mut entries = Vec::with_capacity(obj.len());
                for (key, element) in obj {
                    // C# charges an extra `maxStackSize--` per map entry (before
                    // the value) on top of the value's own item cost.
                    if *remaining == 0 {
                        return Err(CoreError::other("Max stack size reached"));
                    }
                    *remaining -= 1;
                    let key_item = StackItem::from_byte_string(key.as_bytes());
                    let value_item =
                        Self::deserialize_internal(element, depth + 1, max_depth, remaining)?;
                    entries.push((key_item, value_item));
                }
                Ok(StackItem::Map(MapItem::new_untracked(entries)))
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/json_serializer.rs"]
mod tests;

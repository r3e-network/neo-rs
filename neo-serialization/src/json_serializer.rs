#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

use neo_vm::stack_item::{Array, Map as MapItem, Struct};
use neo_vm::StackItem;
use neo_vm_rs::StackItemType;
use num_bigint::BigInt;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::HashSet;

/// JSON serialization utilities for VM stack items.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Maximum safe integer representable without loss in JSON.
    pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;
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
    pub fn serialize_to_byte_array(item: &StackItem, max_size: u32) -> Result<Vec<u8>, String> {
        let json = Self::serialize_to_json(item)?;
        let payload = Self::encode_value_csharp_compatible(&json);
        if payload.len() > max_size as usize {
            return Err("JSON output too large".to_string());
        }
        Ok(payload)
    }

    /// Encodes a `serde_json::Value` to UTF-8 JSON bytes byte-identically with
    /// C# Neo's `System.Text.Json` default output (`JavaScriptEncoder.Default`).
    ///
    /// Delegates to [`neo_json::escape`], whose `CSharpEscapeFormatter` reproduces
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
        neo_json::escape::to_vec(value, false)
            .expect("serde_json::Value serializes to an in-memory buffer infallibly")
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
                    .filter(|v| (Self::MIN_SAFE_INTEGER..=Self::MAX_SAFE_INTEGER).contains(v))
                    .ok_or_else(|| "Integer out of safe JSON range".to_string())?;
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
            StackItem::Pointer(_) => Err("Cannot serialize Pointer to JSON".to_string()),
            StackItem::InteropInterface(_) => {
                Err("Cannot serialize InteropInterface to JSON".to_string())
            }
        }
    }

    fn serialize_compound_array(
        array: &Array,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        let identity = (array.id(), StackItemType::Array);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
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
    ) -> Result<JsonValue, String> {
        let identity = (structure.id(), StackItemType::Struct);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
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
    ) -> Result<JsonValue, String> {
        let identity = (map.id(), StackItemType::Map);
        if !seen.insert(identity) {
            return Err("Circular reference detected".to_string());
        }

        let mut result = JsonMap::new();
        for (key, value) in map.iter() {
            let key_bytes = match &key {
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
                        .ok_or_else(|| "Invalid JSON number".to_string())?;
                    if f.fract() != 0.0 {
                        return Err("Decimal value is not allowed".to_string());
                    }
                    // `{f}` is the shortest round-trippable decimal in fixed (never
                    // scientific) notation — matching the integer value C# obtains
                    // from `BigInteger.Parse(double.ToString())`, not the double's
                    // exact stored value (which `{:.0}` would give).
                    let big = format!("{f}")
                        .parse::<BigInt>()
                        .map_err(|_| "Invalid JSON integer".to_string())?;
                    Ok(StackItem::from_int(big))
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
                // so iterating `obj` already yields entries in source order. Build a
                // Vec<(K, V)> (not a BTreeMap, which would alphabetically sort the keys)
                // and hand it straight to the ordered map item.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ser(item: &StackItem) -> String {
        let bytes = JsonSerializer::serialize_to_byte_array(item, 1 << 20).expect("serialize");
        String::from_utf8(bytes).expect("ascii/utf8 output")
    }

    fn de(json: &str) -> Result<StackItem, String> {
        JsonSerializer::deserialize(json.as_bytes(), 64)
    }

    #[test]
    fn serialize_matches_csharp_stdlib_vectors() {
        // C# UT_StdLib.Json_Serialize.
        assert_eq!(ser(&StackItem::from_int(BigInt::from(5))), "5");
        assert_eq!(ser(&StackItem::from_bool(true)), "true");
        assert_eq!(ser(&StackItem::from_byte_string(b"test".to_vec())), "\"test\"");
        assert_eq!(ser(&StackItem::null()), "null");
        // Map{"key":"value"} (built via deserialize) round-trips compactly.
        assert_eq!(ser(&de(r#"{"key":"value"}"#).unwrap()), r#"{"key":"value"}"#);
    }

    #[test]
    fn serialize_escapes_like_system_text_json() {
        // JavaScriptEncoder.Default: quote -> ", '<'/'>' -> </>,
        // all non-ASCII -> \uXXXX (uppercase), but short forms for \n \t \\.
        assert_eq!(ser(&StackItem::from_byte_string(b"a\"b".to_vec())), "\"a\\u0022b\"");
        assert_eq!(
            ser(&StackItem::from_byte_string("<x>".as_bytes().to_vec())),
            "\"\\u003Cx\\u003E\""
        );
        assert_eq!(ser(&StackItem::from_byte_string("中".as_bytes().to_vec())), "\"\\u4E2D\"");
        assert_eq!(ser(&StackItem::from_byte_string(b"\n\t\\".to_vec())), r#""\n\t\\""#);
    }

    #[test]
    fn serialize_rejects_out_of_safe_range_integer() {
        // C# throws when the integer leaves the JS safe-integer range.
        let too_big = BigInt::from(JsonSerializer::MAX_SAFE_INTEGER) + 1;
        assert!(JsonSerializer::serialize_to_byte_array(&StackItem::from_int(too_big), 1 << 20).is_err());
    }

    #[test]
    fn deserialize_matches_csharp_vectors() {
        // C# UT_StdLib.Json_Deserialize: "123" -> 123, "null" -> Null,
        // "***" -> fault, "123.45" -> fault ("no decimals"). Verified by
        // re-serializing (round-trip) so no StackItem accessor is needed.
        assert!(matches!(de("null").unwrap(), StackItem::Null));
        assert_eq!(ser(&de("123").unwrap()), "123");
        // UT_JsonSerializer.Numbers: integer-valued scientific float -> integer.
        assert_eq!(ser(&de("200.500000E+005").unwrap()), "20050000");
        assert!(de("123.45").is_err(), "fractional value is rejected");
        assert!(de("***").is_err(), "invalid JSON is rejected");
        // Structural round-trips (string / array / object key order).
        assert_eq!(ser(&de(r#""test""#).unwrap()), r#""test""#);
        assert_eq!(ser(&de("[1,true,null]").unwrap()), "[1,true,null]");
        assert_eq!(ser(&de(r#"{"b":1,"a":2}"#).unwrap()), r#"{"b":1,"a":2}"#);
    }
}

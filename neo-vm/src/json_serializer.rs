#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

// Old wrapper types removed - StackValue compounds are flat Vecs now
use crate::StackItem;
use crate::StackItemType;
use num_bigint::BigInt;
use num_traits::{FromPrimitive, ToPrimitive};
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::HashSet;

/// JSON serialization utilities for VM stack items.
pub struct JsonSerializer;

impl JsonSerializer {
    /// Maximum safe integer representable without loss in JSON.
    pub const MAX_SAFE_INTEGER: i64 = 9007199254740991;
    /// Minimum safe integer representable without loss in JSON.
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
    ///
    /// The value tree is walked on an explicit task stack, so a payload as deep
    /// as the stack item tree it came from is written without consuming native
    /// stack.
    fn write_json_value(value: &JsonValue, out: &mut Vec<u8>) {
        enum Task<'a> {
            /// Write this value.
            Value(&'a JsonValue),
            /// Write this object key, quoted and escaped.
            Key(&'a str),
            /// Write this literal byte run.
            Literal(&'static [u8]),
        }

        let mut tasks = vec![Task::Value(value)];
        while let Some(task) = tasks.pop() {
            match task {
                Task::Literal(bytes) => out.extend_from_slice(bytes),
                Task::Key(key) => Self::write_json_string(key, out),
                Task::Value(value) => match value {
                    JsonValue::Null => out.extend_from_slice(b"null"),
                    JsonValue::Bool(true) => out.extend_from_slice(b"true"),
                    JsonValue::Bool(false) => out.extend_from_slice(b"false"),
                    JsonValue::Number(number) => {
                        out.extend_from_slice(number.to_string().as_bytes());
                    }
                    JsonValue::String(text) => Self::write_json_string(text, out),
                    JsonValue::Array(items) => {
                        out.push(b'[');
                        tasks.push(Task::Literal(b"]"));
                        for (index, item) in items.iter().enumerate().rev() {
                            tasks.push(Task::Value(item));
                            if index > 0 {
                                tasks.push(Task::Literal(b","));
                            }
                        }
                    }
                    JsonValue::Object(entries) => {
                        out.push(b'{');
                        tasks.push(Task::Literal(b"}"));
                        for (index, (key, item)) in entries.iter().enumerate().rev() {
                            tasks.push(Task::Value(item));
                            tasks.push(Task::Literal(b":"));
                            tasks.push(Task::Key(key));
                            if index > 0 {
                                tasks.push(Task::Literal(b","));
                            }
                        }
                    }
                },
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
                '"' => out.extend_from_slice(b"\\u0022"),
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
                        let _ = std::io::Write::write_fmt(out, format_args!("\\u{cp:04X}"));
                    } else {
                        // Encode supplementary chars as UTF-16 surrogate pair.
                        let v = cp - 0x10000;
                        let hi = 0xD800 + (v >> 10);
                        let lo = 0xDC00 + (v & 0x3FF);
                        let _ =
                            std::io::Write::write_fmt(out, format_args!("\\u{hi:04X}\\u{lo:04X}"));
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

    /// Walks the item on an explicit frame stack.
    ///
    /// `seen` holds the compounds on the current path, exactly as the previous
    /// recursive form did, so a chain nested as deep as `MaxStackSize`
    /// serializes without consuming native stack.
    fn serialize_internal(
        item: &StackItem,
        seen: &mut HashSet<(usize, StackItemType)>,
    ) -> Result<JsonValue, String> {
        /// A compound whose elements are being serialized.
        enum Frame {
            /// An `Array` or `Struct` under construction.
            List {
                identity: (usize, StackItemType),
                source: std::vec::IntoIter<StackItem>,
                values: Vec<JsonValue>,
            },
            /// A `Map` under construction. Only the values are serialized; the
            /// key of each entry is validated and turned into a string first.
            Map {
                identity: (usize, StackItemType),
                source: std::vec::IntoIter<(StackItem, StackItem)>,
                entries: JsonMap<String, JsonValue>,
                pending_key: Option<String>,
            },
        }

        /// Serializes an item that has no children of its own; `None` means the
        /// item is a compound and needs a frame.
        fn leaf(item: &StackItem) -> Result<Option<JsonValue>, String> {
            match item {
                StackItem::Null => Ok(Some(JsonValue::Null)),
                StackItem::Boolean(value) => Ok(Some(JsonValue::Bool(*value))),
                StackItem::Integer(integer) => {
                    let int_value = integer
                        .to_bigint()
                        .to_i64()
                        .ok_or_else(|| "Integer out of safe JSON range".to_string())?;
                    if !(JsonSerializer::MIN_SAFE_INTEGER..=JsonSerializer::MAX_SAFE_INTEGER)
                        .contains(&int_value)
                    {
                        return Err("Integer out of safe JSON range".to_string());
                    }
                    Ok(Some(JsonValue::Number(JsonNumber::from(int_value))))
                }
                StackItem::ByteString(bytes) => Ok(Some(JsonValue::String(
                    String::from_utf8_lossy(bytes).to_string(),
                ))),
                StackItem::Buffer(buffer) => {
                    Ok(Some(JsonValue::String(buffer.with_data(|data| {
                        String::from_utf8_lossy(data).to_string()
                    }))))
                }
                StackItem::Pointer(_) => Err("Cannot serialize Pointer to JSON".to_string()),
                StackItem::InteropInterface(_) => {
                    Err("Cannot serialize InteropInterface to JSON".to_string())
                }
                _ => Ok(None),
            }
        }

        /// Opens the frame of a compound, marking it as being serialized.
        fn open(
            item: &StackItem,
            seen: &mut HashSet<(usize, StackItemType)>,
        ) -> Result<Frame, String> {
            match item {
                StackItem::Array(array) => {
                    let identity = (array.id(), StackItemType::Array);
                    if !seen.insert(identity) {
                        return Err(
                            "Cyclic compound references cannot be serialized to JSON.".to_string()
                        );
                    }
                    Ok(Frame::List {
                        identity,
                        source: array.items().into_iter(),
                        values: Vec::with_capacity(array.len()),
                    })
                }
                StackItem::Struct(structure) => {
                    let identity = (structure.id(), StackItemType::Struct);
                    if !seen.insert(identity) {
                        return Err(
                            "Cyclic compound references cannot be serialized to JSON.".to_string()
                        );
                    }
                    Ok(Frame::List {
                        identity,
                        source: structure.items().into_iter(),
                        values: Vec::with_capacity(structure.len()),
                    })
                }
                StackItem::Map(map) => {
                    let identity = (map.id(), StackItemType::Map);
                    if !seen.insert(identity) {
                        return Err(
                            "Cyclic compound references cannot be serialized to JSON.".to_string()
                        );
                    }
                    Ok(Frame::Map {
                        identity,
                        source: map.iter(),
                        entries: JsonMap::new(),
                        pending_key: None,
                    })
                }
                _ => unreachable!("only compound items are opened"),
            }
        }

        fn next_child(frame: &mut Frame) -> Result<Option<StackItem>, String> {
            match frame {
                Frame::List { source, .. } => Ok(source.next()),
                Frame::Map {
                    source,
                    pending_key,
                    ..
                } => {
                    let Some((key, value)) = source.next() else {
                        return Ok(None);
                    };
                    let key_bytes = match key {
                        StackItem::ByteString(bytes) => bytes,
                        StackItem::Buffer(buffer) => buffer.data(),
                        _ => return Err("Map key must be a byte string".to_string()),
                    };
                    *pending_key = Some(
                        String::from_utf8(key_bytes)
                            .map_err(|_| "Invalid UTF-8 in map key".to_string())?,
                    );
                    Ok(Some(value))
                }
            }
        }

        fn accept(frame: &mut Frame, value: JsonValue) {
            match frame {
                Frame::List { values, .. } => values.push(value),
                Frame::Map {
                    entries,
                    pending_key,
                    ..
                } => {
                    if let Some(key) = pending_key.take() {
                        entries.insert(key, value);
                    }
                }
            }
        }

        fn close(frame: Frame, seen: &mut HashSet<(usize, StackItemType)>) -> JsonValue {
            match frame {
                Frame::List {
                    identity, values, ..
                } => {
                    seen.remove(&identity);
                    JsonValue::Array(values)
                }
                Frame::Map {
                    identity, entries, ..
                } => {
                    seen.remove(&identity);
                    JsonValue::Object(entries)
                }
            }
        }

        let mut frames: Vec<Frame> = Vec::new();
        let mut pending = Some(item.clone());
        let mut converted = JsonValue::Null;

        loop {
            if let Some(item) = pending.take() {
                if let Some(leaf) = leaf(&item)? {
                    converted = leaf;
                } else {
                    let mut frame = open(&item, seen)?;
                    let first = next_child(&mut frame)?;
                    frames.push(frame);
                    match first {
                        Some(child) => pending = Some(child),
                        None => {
                            let frame = frames.pop().expect("open frame");
                            converted = close(frame, seen);
                        }
                    }
                    continue;
                }
            }

            if frames.is_empty() {
                return Ok(converted);
            }
            let value = std::mem::replace(&mut converted, JsonValue::Null);
            accept(frames.last_mut().expect("open frame"), value);

            let child = match frames.last_mut() {
                Some(frame) => next_child(frame)?,
                None => None,
            };
            match child {
                Some(child) => pending = Some(child),
                None => {
                    let frame = frames.pop().expect("open frame");
                    converted = close(frame, seen);
                }
            }
        }
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
                } else if let Some(f) = n.as_f64() {
                    if !f.is_finite() || f % 1.0 != 0.0 {
                        Err("Decimal value is not allowed".to_string())
                    } else {
                        // C# parity: Utf8JsonReader.GetDouble() -> JNumber -> (BigInteger)val.
                        // Converts integer-valued floating point numbers (e.g. 1e21, 1.0, 1e5)
                        // into BigInt.
                        BigInt::from_f64(f)
                            .map(StackItem::from_int)
                            .ok_or_else(|| "Unsupported JSON number".to_string())
                    }
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
                Ok(StackItem::from_map(entries))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_numbers() {
        let cases = [
            ("0", Some(BigInt::from(0))),
            ("123", Some(BigInt::from(123))),
            ("-456", Some(BigInt::from(-456))),
            ("1.0", Some(BigInt::from(1))),
            ("1e5", Some(BigInt::from(100000))),
            ("1000000000000000000000", BigInt::from_f64(1e21)),
            ("1.5", None),
            ("-2.7", None),
        ];
        for (json_str, expected) in cases {
            let res = JsonSerializer::deserialize(json_str.as_bytes(), 10);
            match expected {
                Some(exp) => {
                    let item = res.unwrap_or_else(|e| panic!("failed to parse {json_str}: {e}"));
                    assert_eq!(item.as_int().unwrap(), exp, "mismatch for {json_str}");
                }
                None => {
                    assert!(res.is_err(), "expected error for {json_str}, got {res:?}");
                }
            }
        }
    }

    #[test]
    fn csharp_encoding_escapes_quotes_inside_strings_as_unicode() {
        let value = serde_json::json!({
            "Deployment": "{\"p\":\"neoz-20\",\"op\":\"deploy\"}"
        });

        let encoded = JsonSerializer::encode_value_csharp_compatible(&value);

        assert_eq!(
            encoded,
            br#"{"Deployment":"{\u0022p\u0022:\u0022neoz-20\u0022,\u0022op\u0022:\u0022deploy\u0022}"}"#
        );
    }

    #[test]
    fn test_deserialize_block_1497790_payload() {
        let json_payload = br#"{"nft":{"tokenid":"1"},"settings":{"jd_name":"OG","price":1000000000000000000000,"init_supply":80,"is_transferable":true}}"#;
        let item = JsonSerializer::deserialize(json_payload, 10).expect("should deserialize");
        assert!(item.as_map().is_ok());
    }
}

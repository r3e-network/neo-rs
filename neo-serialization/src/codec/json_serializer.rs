#![allow(clippy::mutable_key_type)]

//! JsonSerializer - mirrors `Neo.SmartContract.JsonSerializer`.

use neo_error::{CoreError, CoreResult};
use neo_vm::StackItem;
use neo_vm::StackItemType;
use neo_vm::stack_item::{Array, Map as MapItem, Struct};
use num_bigint::BigInt;
use serde_json::{Map as JsonMap, Number as JsonNumber, Value as JsonValue};
use std::collections::HashSet;

/// Reproduces C#'s `(BigInteger)double` conversion for a finite `f64`: the exact
/// binary value of the IEEE-754 double, truncated toward zero.
///
/// This is the pre-`HF_Basilisk` `JsonSerializer.Deserialize` behavior. C# reads
/// every JSON number as a `double` and, before Basilisk, casts it directly:
/// `return (BigInteger)num.Value;` (JsonSerializer.cs:201). That cast takes the
/// double's *stored* value — e.g. `1e30` becomes `1000000000000000019884624838656`
/// (mantissa*2^exp of the nearest double), NOT the decimal `10^30`.
///
/// The double `sign * mantissa * 2^exp` is reconstructed from its raw bit pattern
/// and, for negative exponents, right-shifted (which truncates toward zero once the
/// sign is re-applied — matching .NET's toward-zero `(BigInteger)double`).
fn bigint_from_double_truncated(d: f64) -> BigInt {
    if d == 0.0 {
        return BigInt::from(0);
    }
    let bits = d.to_bits();
    let sign: i8 = if (bits >> 63) != 0 { -1 } else { 1 };
    let raw_exponent = ((bits >> 52) & 0x7FF) as i64;
    let raw_mantissa = bits & ((1u64 << 52) - 1);
    let (mantissa, exponent) = if raw_exponent == 0 {
        // Subnormal: no implicit leading bit; biased exponent is 1 - 1023 - 52.
        (raw_mantissa, -1074i64)
    } else {
        // Normal: restore the implicit leading 1, unbias, and account for the 52-bit
        // fractional mantissa (1023 exponent bias + 52).
        (raw_mantissa | (1u64 << 52), raw_exponent - 1075)
    };
    let magnitude = BigInt::from(mantissa);
    let magnitude = if exponent >= 0 {
        magnitude << (exponent as usize)
    } else {
        // Right-shifting the non-negative magnitude drops the fractional bits, i.e.
        // truncates toward zero once the sign is re-applied below.
        magnitude >> ((-exponent) as usize)
    };
    if sign < 0 { -magnitude } else { magnitude }
}

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
        let payload = Self::try_encode_value_csharp_compatible(&json)?;
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
        Self::try_encode_value_csharp_compatible(value).unwrap_or_default()
    }

    /// Fallible variant of [`Self::encode_value_csharp_compatible`] for callers
    /// that already return a typed error.
    pub fn try_encode_value_csharp_compatible(value: &JsonValue) -> CoreResult<Vec<u8>> {
        crate::json::escape::to_vec(value, false)
            .map_err(|err| CoreError::other(format!("JSON serialization failed: {err}")))
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
                // C# JsonSerializer decodes ByteString/Buffer values with StrictUTF8
                // (throwOnInvalidBytes=true), so invalid UTF-8 FAULTs the VM. Use
                // strict from_utf8 (mirroring the map-key path) instead of the lossy
                // decode, so an invalid-UTF-8 value cannot succeed here while a C#
                // node faults — a StdLib.jsonSerialize state divergence.
                let string = String::from_utf8(bytes.clone())
                    .map_err(|_| CoreError::other("Invalid UTF-8 in byte string"))?;
                Ok(JsonValue::String(string))
            }
            StackItem::Buffer(buffer) => {
                let string = String::from_utf8(buffer.data())
                    .map_err(|_| CoreError::other("Invalid UTF-8 in byte string"))?;
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
    ///
    /// `basilisk_active` selects the JSON-number → VM Integer conversion, gated on
    /// `HF_Basilisk` exactly as C# `JsonSerializer.Deserialize` gates on
    /// `engine.IsHardforkEnabled(Hardfork.HF_Basilisk)` (JsonSerializer.cs:197).
    /// Pre-Basilisk (`false`) reproduces C#'s `(BigInteger)double` truncating cast of
    /// the double's exact stored value; post-Basilisk (`true`) parses the double's
    /// shortest round-trip decimal, so e.g. `1e30` yields
    /// `1000000000000000019884624838656` before Basilisk and `10^30` after. The
    /// caller MUST pass the flag for the block height being replayed, or replay
    /// diverges from C# on numbers whose magnitude exceeds 2^53.
    pub fn deserialize(
        json: &[u8],
        max_depth: usize,
        max_items: usize,
        basilisk_active: bool,
    ) -> CoreResult<StackItem> {
        let value: JsonValue =
            serde_json::from_slice(json).map_err(|e| CoreError::other(e.to_string()))?;
        Self::deserialize_from_json(&value, max_depth, max_items, basilisk_active)
    }

    /// Deserializes a [`JsonValue`] into a stack item (see [`deserialize`] for the
    /// limit and `basilisk_active` semantics).
    ///
    /// [`deserialize`]: Self::deserialize
    pub fn deserialize_from_json(
        value: &JsonValue,
        max_depth: usize,
        max_items: usize,
        basilisk_active: bool,
    ) -> CoreResult<StackItem> {
        let mut remaining = max_items;
        Self::deserialize_internal(value, 0, max_depth, &mut remaining, basilisk_active)
    }

    fn deserialize_internal(
        value: &JsonValue,
        depth: usize,
        max_depth: usize,
        remaining: &mut usize,
        basilisk_active: bool,
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
                // C# JsonSerializer.Deserialize treats EVERY JNumber as a `double`
                // (Neo.Json `JNumber.Value` is a double parsed via GetDouble(), with
                // no MAX_SAFE_INTEGER guard). The fractional check `num.Value % 1 != 0`
                // (JsonSerializer.cs:196) runs BEFORE the hardfork gate, so a
                // non-integral double FAULTS both pre- and post-Basilisk. Only when
                // the double is integral do the two eras diverge on the conversion.
                //
                // serde_json parses i64/u64 literals exactly, so to match C# we route
                // any number through the f64 the double would have held. Values within
                // +/-2^53 are represented exactly by f64, so for those the exact path
                // is kept (identical result in both eras, no float round-trip needed).
                const MAX_EXACT: i128 = 1i128 << 53;
                let exact = n
                    .as_i64()
                    .filter(|v| i128::from(*v).unsigned_abs() <= MAX_EXACT as u128)
                    .map(BigInt::from)
                    .or_else(|| {
                        n.as_u64()
                            .filter(|v| u128::from(*v) <= MAX_EXACT as u128)
                            .map(BigInt::from)
                    });
                if let Some(big) = exact {
                    // Within +/-2^53 both eras agree (the double is the exact integer,
                    // and its shortest decimal parses back to the same value).
                    return Ok(StackItem::from_int(big));
                }
                // Large integer (or non-integer): reproduce C#'s lossy double.
                let f = n
                    .as_f64()
                    .ok_or_else(|| CoreError::other("Invalid JSON number"))?;
                if f.fract() != 0.0 {
                    // C# `num.Value % 1 != 0` faults regardless of hardfork.
                    return Err(CoreError::other("Decimal value is not allowed"));
                }
                let big = if basilisk_active {
                    // Post-Basilisk: `BigInteger.Parse(num.Value.ToString(...))`. `{f}`
                    // is the shortest round-trippable decimal in fixed (never
                    // scientific) notation — the integer value C# obtains from
                    // `BigInteger.Parse(double.ToString())`, NOT the double's exact
                    // stored value. So `1e30` -> 10^30.
                    format!("{f}")
                        .parse::<BigInt>()
                        .map_err(|_| CoreError::other("Invalid JSON integer"))?
                } else {
                    // Pre-Basilisk: `(BigInteger)num.Value` — the double's EXACT stored
                    // binary value, truncated toward zero. So `1e30` ->
                    // 1000000000000000019884624838656.
                    bigint_from_double_truncated(f)
                };
                Ok(StackItem::from_int(big))
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
                        basilisk_active,
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
                    let value_item = Self::deserialize_internal(
                        element,
                        depth + 1,
                        max_depth,
                        remaining,
                        basilisk_active,
                    )?;
                    entries.push((key_item, value_item));
                }
                Ok(StackItem::Map(MapItem::new_untracked(entries)))
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/codec/json_serializer.rs"]
mod tests;

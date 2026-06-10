//! StdLib native contract (id -2).
//!
//! Implements the C# `Neo.SmartContract.Native.StdLib` surface, dispatched
//! through the [`NativeContract`] trait: Base64/Base58 (incl. `base64Url*` and
//! Base58Check), hex, `itoa`/`atoi` (decimal + .NET two's-complement hex),
//! `memoryCompare` / `memorySearch`, `stringSplit`, `strLen` (.NET
//! `StringInfo` text elements, via [`crate::dotnet_text_segmentation`] and an
//! in-repo .NET oracle fixture), the binary `serialize`/`deserialize`
//! (BinarySerializer), and `jsonSerialize`/`jsonDeserialize`
//! (System.Text.Json byte-exact, via [`neo_serialization::JsonSerializer`]).
//! Every method declared below is byte-for-byte C# parity with a real
//! implementation.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::{Base58, Base64, Hex};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use neo_serialization::{BinarySerializer, JsonSerializer};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use crate::dotnet_text_segmentation::text_element_count;
use crate::hashes::STDLIB_HASH;

/// C# `StdLib.MaxInputLength` — the `[MaxLength]` cap on string/byte inputs.
const MAX_INPUT_LENGTH: usize = 1024;

/// Lazily-initialised script-hash handle for the StdLib contract.
pub static STDLIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *STDLIB_HASH);

/// The StdLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdLib;

impl StdLib {
    /// Stable native contract id (matches C# `StdLib`).
    pub const ID: i32 = -2;

    /// Construct a new `StdLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the StdLib script hash.
    pub fn script_hash() -> UInt160 {
        *STDLIB_HASH_REF
    }
}

fn arg_bytes<'a>(args: &'a [Vec<u8>], method: &str) -> CoreResult<&'a [u8]> {
    args.first().map(Vec::as_slice).ok_or_else(|| {
        CoreError::invalid_operation(format!("StdLib::{method} requires one argument"))
    })
}

/// Interprets the single argument as a native string (a VM ByteString carrying
/// UTF-8 bytes).
fn arg_str(args: &[Vec<u8>], method: &str) -> CoreResult<String> {
    String::from_utf8(arg_bytes(args, method)?.to_vec()).map_err(|_| {
        CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
    })
}

/// Pure dispatch for StdLib's stateless methods, split out so it can be unit
/// tested without constructing an [`ApplicationEngine`]. Returns `None` for an
/// unknown method so [`StdLib::invoke`] can emit a precise error.
fn dispatch(method: &str, args: &[Vec<u8>]) -> Option<CoreResult<Vec<u8>>> {
    let result = match method {
        // Encoders: ByteArray -> String (returned as UTF-8 bytes).
        "base64Encode" => arg_bytes(args, method).map(|b| Base64::encode(b).into_bytes()),
        // base64Decode: String -> ByteArray (C# Convert.FromBase64String).
        "base64Decode" => base64_decode_impl(args),
        // base64Url* (HF_Echidna): String <-> String, URL-safe alphabet, no padding.
        "base64UrlEncode" => base64_url_encode_impl(args),
        "base64UrlDecode" => base64_url_decode_impl(args),
        // hexEncode/hexDecode (HF_Faun): ByteArray <-> lowercase hex String.
        "hexEncode" => hex_encode_impl(args),
        "hexDecode" => hex_decode_impl(args),
        "base58Encode" => arg_bytes(args, method).map(|b| Base58::encode(b).into_bytes()),
        "base58CheckEncode" => {
            arg_bytes(args, method).map(|b| Base58::encode_check(b).into_bytes())
        }
        // Decoders: String -> ByteArray. Invalid input faults the call, matching
        // C# (Base58 throws on a bad alphabet / checksum).
        "base58Decode" => arg_str(args, method).and_then(|s| {
            Base58::decode(&s)
                .map_err(|e| CoreError::invalid_operation(format!("StdLib::base58Decode: {e}")))
        }),
        "base58CheckDecode" => arg_str(args, method).and_then(|s| {
            Base58::decode_check(&s).map_err(|e| {
                CoreError::invalid_operation(format!("StdLib::base58CheckDecode: {e}"))
            })
        }),
        // memoryCompare(a, b) -> Math.Sign(a.SequenceCompareTo(b)) as Integer.
        // Rust slice `cmp` is the same lexicographic-then-length ordering.
        "memoryCompare" => match (args.first(), args.get(1)) {
            (Some(a), Some(b)) => {
                let sign: i32 = match a.as_slice().cmp(b.as_slice()) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                };
                Ok(BigInt::from(sign).to_signed_bytes_le())
            }
            _ => Err(CoreError::invalid_operation(
                "StdLib::memoryCompare requires two arguments".to_string(),
            )),
        },
        // memorySearch(mem, value[, start[, backward]]) -> Integer index or -1.
        "memorySearch" => memory_search_impl(args),
        // itoa(value[, base]) -> String; atoi(value[, base]) -> Integer.
        "itoa" => itoa_impl(args),
        "atoi" => atoi_impl(args),
        // stringSplit(str, separator[, removeEmptyEntries]) -> Array of String.
        "stringSplit" => string_split_impl(args),
        // strLen(str) -> Integer: the .NET StringInfo text-element count.
        "strLen" => str_len_impl(args),
        // serialize(item) -> the item's BinarySerializer bytes. The `Any`-typed
        // arg is already BinarySerialized by the engine, so C#
        // `BinarySerializer.Serialize(item)` is exactly args[0].
        "serialize" => arg_bytes(args, method).map(<[u8]>::to_vec),
        // deserialize(data) -> the StackItem. We validate the payload here
        // (C# faults on malformed input, whereas the engine's Any-return decode
        // falls back to a raw ByteString) and hand the bytes back for that decode.
        "deserialize" => arg_bytes(args, method).and_then(|data| {
            BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
                .map(|_| data.to_vec())
                .map_err(|e| CoreError::invalid_operation(format!("StdLib::deserialize: {e}")))
        }),
        // jsonSerialize(item) -> JSON bytes (System.Text.Json byte-exact). The
        // `Any`-typed arg arrives BinarySerialized by the engine, so decode it to
        // a StackItem first, then JSON-encode bounded by the VM item-size limit
        // (C# `SerializeToByteArray(item, engine.Limits.MaxItemSize)`).
        "jsonSerialize" => arg_bytes(args, method).and_then(|data| {
            let limits = ExecutionEngineLimits::default();
            let item = BinarySerializer::deserialize(data, &limits, None)
                .map_err(|e| CoreError::invalid_operation(format!("StdLib::jsonSerialize: {e}")))?;
            JsonSerializer::serialize_to_byte_array(&item, limits.max_item_size)
                .map_err(|e| CoreError::invalid_operation(format!("StdLib::jsonSerialize: {e}")))
        }),
        // jsonDeserialize(json) -> the StackItem, re-encoded as BinarySerializer
        // bytes for the engine's `Any`-return decode. Depth 10 + MaxStackSize
        // match C# (`JToken.Parse(json, 10)` + `engine.Limits`).
        "jsonDeserialize" => arg_bytes(args, method).and_then(|json| {
            let limits = ExecutionEngineLimits::default();
            let item = JsonSerializer::deserialize(json, 10, limits.max_stack_size as usize)
                .map_err(|e| {
                    CoreError::invalid_operation(format!("StdLib::jsonDeserialize: {e}"))
                })?;
            BinarySerializer::serialize(&item, &limits).map_err(|e| {
                CoreError::invalid_operation(format!("StdLib::jsonDeserialize: {e}"))
            })
        }),
        _ => return None,
    };
    Some(result)
}

/// C# `StdLib.MemorySearch` (its 3 overloads dispatch by argument count):
/// forward search returns `mem[start..].IndexOf(value) + start` (or -1);
/// backward search returns `mem[0..start].LastIndexOf(value)` (or -1).
fn memory_search_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let mem = args.first().map(Vec::as_slice).ok_or_else(|| {
        CoreError::invalid_operation("StdLib::memorySearch requires (mem, value)")
    })?;
    let value = args.get(1).map(Vec::as_slice).ok_or_else(|| {
        CoreError::invalid_operation("StdLib::memorySearch requires (mem, value)")
    })?;
    let start = match args.get(2) {
        Some(b) => BigInt::from_signed_bytes_le(b).to_usize().ok_or_else(|| {
            CoreError::invalid_operation("StdLib::memorySearch: start out of range")
        })?,
        None => 0,
    };
    // C# `AsSpan(start)` / `AsSpan(0, start)` throw when start exceeds the length.
    if start > mem.len() {
        return Err(CoreError::invalid_operation(
            "StdLib::memorySearch: start out of range",
        ));
    }
    let backward = args.get(3).map(|b| b.iter().any(|x| *x != 0)).unwrap_or(false);
    Ok(BigInt::from(memory_search(mem, value, start, backward)).to_signed_bytes_le())
}

fn memory_search(mem: &[u8], value: &[u8], start: usize, backward: bool) -> i64 {
    if backward {
        last_index_of(&mem[..start], value)
    } else {
        match index_of(&mem[start..], value) {
            Some(i) => (i + start) as i64,
            None => -1,
        }
    }
}

/// First index of `needle` in `haystack`, matching .NET `Span.IndexOf`
/// (an empty needle is found at 0).
fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Last index of `needle` in `haystack` (or -1), matching .NET `Span.LastIndexOf`
/// (an empty needle is reported at `haystack.len()`).
fn last_index_of(haystack: &[u8], needle: &[u8]) -> i64 {
    if needle.is_empty() {
        return haystack.len() as i64;
    }
    if needle.len() > haystack.len() {
        return -1;
    }
    haystack
        .windows(needle.len())
        .rposition(|w| w == needle)
        .map_or(-1, |i| i as i64)
}

/// Reads an optional integer `base` argument (C# StdLib's `@base` overload),
/// defaulting to 10 when absent. Integer args arrive as signed little-endian.
fn optional_base(args: &[Vec<u8>], index: usize, method: &str) -> CoreResult<i64> {
    match args.get(index) {
        None => Ok(10),
        Some(bytes) => BigInt::from_signed_bytes_le(bytes).to_i64().ok_or_else(|| {
            CoreError::invalid_argument(format!("StdLib::{method}: base out of range"))
        }),
    }
}

/// C# `StdLib.Base64Decode` = `Convert.FromBase64String`: strip the four
/// whitespace characters .NET tolerates ({space, `\t`, `\n`, `\r`}), then
/// strict-decode the remainder (any other character — including other
/// whitespace — faults). Enforces the C# `[MaxLength(1024)]` cap on the input.
fn base64_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "base64Decode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64Decode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::base64Decode: argument is not valid UTF-8".to_string())
    })?;
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
        .collect();
    Base64::decode_strict(&stripped)
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::base64Decode: {e}")))
}

/// C# `StdLib.Base64UrlEncode(data)` (HF_Echidna) = `Base64UrlEncoder.Encode`:
/// encodes the UTF-8 bytes of the input string into a URL-safe, unpadded base64
/// string. Enforces the C# `[MaxLength(1024)]` cap on the input.
fn base64_url_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "base64UrlEncode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64UrlEncode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    Ok(Base64::url_encode_no_pad(raw).into_bytes())
}

/// C# `StdLib.Base64UrlDecode(s)` (HF_Echidna) = `Base64UrlEncoder.Decode`: strip
/// the four whitespace characters .NET tolerates ({space, `\t`, `\n`, `\r`}),
/// then strict URL-safe-no-padding decode. The decoded bytes are returned as the
/// `String` result. Enforces the C# `[MaxLength(1024)]` cap on the input.
fn base64_url_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "base64UrlDecode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::base64UrlDecode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation(
            "StdLib::base64UrlDecode: argument is not valid UTF-8".to_string(),
        )
    })?;
    let stripped: String = value
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r'))
        .collect();
    Base64::url_decode_no_pad_strict(&stripped)
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::base64UrlDecode: {e}")))
}

/// C# `StdLib.Itoa(value[, base])`: base 10 -> `BigInteger.ToString()` (decimal),
/// base 16 -> `BigInteger.ToString("x")` (lowercase two's-complement hex).
/// Any other base throws `ArgumentOutOfRangeException`.
fn itoa_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let value = BigInt::from_signed_bytes_le(arg_bytes(args, "itoa")?);
    let text = match optional_base(args, 1, "itoa")? {
        10 => value.to_str_radix(10),
        16 => dotnet_bigint_to_hex(&value),
        other => {
            return Err(CoreError::invalid_argument(format!(
                "StdLib::itoa: invalid base: {other}"
            )))
        }
    };
    Ok(text.into_bytes())
}

/// C# `StdLib.Atoi(value[, base])`: base 10 -> `BigInteger.Parse(AllowLeadingSign)`,
/// base 16 -> `BigInteger.Parse(AllowHexSpecifier)` (two's-complement). Enforces
/// the C# `[MaxLength(1024)]` cap on the input. Any other base throws.
fn atoi_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "atoi")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::atoi: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::atoi: argument is not valid UTF-8".to_string())
    })?;
    let parsed = match optional_base(args, 1, "atoi")? {
        10 => parse_dotnet_decimal(value)?,
        16 => parse_dotnet_hex(value)?,
        other => {
            return Err(CoreError::invalid_argument(format!(
                "StdLib::atoi: invalid base: {other}"
            )))
        }
    };
    Ok(parsed.to_signed_bytes_le())
}

/// C# `StdLib.HexEncode(bytes)` (HF_Faun) = `bytes.ToHexString()`: lowercase hex,
/// no prefix. Enforces the C# `[MaxLength(1024)]` cap on the input.
fn hex_encode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "hexEncode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::hexEncode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    Ok(Hex::encode(raw).into_bytes())
}

/// C# `StdLib.HexDecode(str)` (HF_Faun) = `str.HexToBytes()` (`Convert.FromHexString`):
/// case-insensitive hex, length must be even, no prefix/whitespace. Enforces the
/// C# `[MaxLength(1024)]` cap on the input.
fn hex_decode_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "hexDecode")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::hexDecode: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::hexDecode: argument is not valid UTF-8".to_string())
    })?;
    Hex::decode(value).map_err(|e| CoreError::invalid_operation(format!("StdLib::hexDecode: {e}")))
}

/// C# `StdLib.StringSplit(str, separator[, removeEmptyEntries])` = `String.Split`:
/// split `str` on each occurrence of `separator`, keeping empty entries unless
/// `removeEmptyEntries` is true. An empty separator yields `[str]` (the whole
/// string), matching .NET's `string.Split(string)` overload. Enforces the C#
/// `[MaxLength(1024)]` cap on `str`. Returns a VM Array of ByteStrings
/// (BinarySerialized; the engine deserializes it for the `Array` return type).
fn string_split_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "stringSplit")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::stringSplit: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::stringSplit: argument is not valid UTF-8".to_string())
    })?;
    let separator = match args.get(1) {
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| {
            CoreError::invalid_operation(
                "StdLib::stringSplit: separator is not valid UTF-8".to_string(),
            )
        })?,
        None => {
            return Err(CoreError::invalid_operation(
                "StdLib::stringSplit requires (str, separator)".to_string(),
            ))
        }
    };
    let remove_empty = args.get(2).map(|b| b.iter().any(|x| *x != 0)).unwrap_or(false);

    let parts: Vec<&str> = if separator.is_empty() {
        // .NET `string.Split("")` returns the whole string as a single element.
        vec![value]
    } else {
        value.split(separator).collect()
    };
    let items: Vec<StackItem> = parts
        .into_iter()
        .filter(|part| !remove_empty || !part.is_empty())
        .map(|part| StackItem::from_byte_string(part.as_bytes().to_vec()))
        .collect();

    BinarySerializer::serialize(&StackItem::from_array(items), &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("StdLib::stringSplit: {e}")))
}

/// C# `StdLib.StrLen(str)`: the number of text elements in the string, i.e.
/// .NET `StringInfo` extended grapheme clusters (UAX #29 minus GB9c over the
/// .NET runtime's break-property snapshot; see
/// [`crate::dotnet_text_segmentation`]). Enforces the C# `[MaxLength(1024)]`
/// cap on the raw input bytes; invalid UTF-8 faults the call, matching the C#
/// `StrictUTF8` string conversion.
fn str_len_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
    let raw = arg_bytes(args, "strLen")?;
    if raw.len() > MAX_INPUT_LENGTH {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::strLen: input exceeds maximum length ({MAX_INPUT_LENGTH})"
        )));
    }
    let value = std::str::from_utf8(raw).map_err(|_| {
        CoreError::invalid_operation("StdLib::strLen: argument is not valid UTF-8".to_string())
    })?;
    Ok(BigInt::from(text_element_count(value)).to_signed_bytes_le())
}

/// Mirrors .NET `BigInteger.ToString("x")`: lowercase, minimal two's-complement
/// hex with a sign-disambiguating leading nibble (a positive value whose top
/// nibble is >= 8 gets a leading `0`; negatives are rendered in two's
/// complement, e.g. `-1` -> "f", `255` -> "0ff", `-256` -> "f00").
fn dotnet_bigint_to_hex(value: &BigInt) -> String {
    if value.sign() == Sign::NoSign {
        return "0".to_string();
    }
    let negative = value.sign() == Sign::Minus;
    let mut hex = String::new();
    for byte in value.to_signed_bytes_be() {
        hex.push_str(&format!("{byte:02x}"));
    }
    let chars: Vec<char> = hex.chars().collect();
    let mut start = 0;
    // Drop redundant leading sign nibbles while the remainder keeps the sign.
    while start + 1 < chars.len() {
        let redundant = if negative {
            chars[start] == 'f' && matches!(chars[start + 1], '8'..='9' | 'a'..='f')
        } else {
            chars[start] == '0' && matches!(chars[start + 1], '0'..='7')
        };
        if redundant {
            start += 1;
        } else {
            break;
        }
    }
    chars[start..].iter().collect()
}

/// Mirrors .NET `BigInteger.Parse(value, NumberStyles.AllowLeadingSign)`: an
/// optional leading `+`/`-` then one or more decimal digits, nothing else
/// (no whitespace, separators, or radix point).
fn parse_dotnet_decimal(value: &str) -> CoreResult<BigInt> {
    let (digits, negative) = match value.as_bytes().first() {
        Some(b'+') => (&value[1..], false),
        Some(b'-') => (&value[1..], true),
        _ => (value, false),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::atoi: '{value}' is not a valid base-10 integer"
        )));
    }
    let magnitude = BigInt::parse_bytes(digits.as_bytes(), 10).ok_or_else(|| {
        CoreError::invalid_operation(format!("StdLib::atoi: '{value}' is not a valid integer"))
    })?;
    Ok(if negative { -magnitude } else { magnitude })
}

/// Mirrors .NET `BigInteger.Parse(value, NumberStyles.AllowHexSpecifier)`:
/// case-insensitive hex digits interpreted as two's-complement (a leading
/// nibble >= 8 makes the value negative, e.g. "ff" -> -1, "0ff" -> 255).
fn parse_dotnet_hex(value: &str) -> CoreResult<BigInt> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CoreError::invalid_operation(format!(
            "StdLib::atoi: '{value}' is not a valid base-16 integer"
        )));
    }
    let lower = value.to_ascii_lowercase();
    let magnitude = BigInt::parse_bytes(lower.as_bytes(), 16).ok_or_else(|| {
        CoreError::invalid_operation(format!("StdLib::atoi: '{value}' is not a valid integer"))
    })?;
    if matches!(lower.as_bytes()[0], b'8'..=b'9' | b'a'..=b'f') {
        Ok(magnitude - (BigInt::from(1) << (4 * lower.len())))
    } else {
        Ok(magnitude)
    }
}

static STDLIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let bytes = ContractParameterType::ByteArray;
    let string = ContractParameterType::String;
    let int = ContractParameterType::Integer;
    let boolean = ContractParameterType::Boolean;
    let array = ContractParameterType::Array;
    vec![
        NativeMethod::new("base64Encode".into(), 1 << 5, true, 0, vec![bytes], string),
        NativeMethod::new("base64Decode".into(), 1 << 5, true, 0, vec![string], bytes),
        NativeMethod::new("base58Encode".into(), 1 << 13, true, 0, vec![bytes], string),
        NativeMethod::new("base58Decode".into(), 1 << 10, true, 0, vec![string], bytes),
        NativeMethod::new("base58CheckEncode".into(), 1 << 16, true, 0, vec![bytes], string),
        NativeMethod::new("base58CheckDecode".into(), 1 << 16, true, 0, vec![string], bytes),
        // serialize(Any) -> ByteArray; deserialize(ByteArray) -> Any.
        NativeMethod::new("serialize".into(), 1 << 12, true, 0, vec![ContractParameterType::Any], bytes),
        NativeMethod::new("deserialize".into(), 1 << 14, true, 0, vec![bytes], ContractParameterType::Any),
        // jsonSerialize(Any) -> ByteArray; jsonDeserialize(ByteArray) -> Any
        // (C# StdLib.cs CpuFees 1<<12 / 1<<14).
        NativeMethod::new("jsonSerialize".into(), 1 << 12, true, 0, vec![ContractParameterType::Any], bytes),
        NativeMethod::new("jsonDeserialize".into(), 1 << 14, true, 0, vec![bytes], ContractParameterType::Any),
        NativeMethod::new("memoryCompare".into(), 1 << 5, true, 0, vec![bytes, bytes], int),
        // memorySearch's 3 C# overloads (dispatched by argument count).
        NativeMethod::new("memorySearch".into(), 1 << 6, true, 0, vec![bytes, bytes], int),
        NativeMethod::new("memorySearch".into(), 1 << 6, true, 0, vec![bytes, bytes, int], int),
        NativeMethod::new(
            "memorySearch".into(),
            1 << 6,
            true,
            0,
            vec![bytes, bytes, int, boolean],
            int,
        ),
        // itoa(value[, base]) -> String; atoi(value[, base]) -> Integer.
        NativeMethod::new("itoa".into(), 1 << 12, true, 0, vec![int], string),
        NativeMethod::new("itoa".into(), 1 << 12, true, 0, vec![int, int], string),
        NativeMethod::new("atoi".into(), 1 << 6, true, 0, vec![string], int),
        NativeMethod::new("atoi".into(), 1 << 6, true, 0, vec![string, int], int),
        // stringSplit(str, separator[, removeEmptyEntries]) -> Array of String.
        NativeMethod::new("stringSplit".into(), 1 << 8, true, 0, vec![string, string], array),
        NativeMethod::new(
            "stringSplit".into(),
            1 << 8,
            true,
            0,
            vec![string, string, boolean],
            array,
        ),
        // strLen(str) -> Integer (count of .NET StringInfo text elements);
        // ungated in C# StdLib.cs, CpuFee 1 << 8.
        NativeMethod::new("strLen".into(), 1 << 8, true, 0, vec![string], int),
        // base64Url* are available from the Echidna hardfork onward.
        NativeMethod::new("base64UrlEncode".into(), 1 << 5, true, 0, vec![string], string)
            .with_active_in(Hardfork::HfEchidna),
        NativeMethod::new("base64UrlDecode".into(), 1 << 5, true, 0, vec![string], string)
            .with_active_in(Hardfork::HfEchidna),
        // hexEncode/hexDecode are available from the Faun hardfork onward.
        NativeMethod::new("hexEncode".into(), 1 << 5, true, 0, vec![bytes], string)
            .with_active_in(Hardfork::HfFaun),
        NativeMethod::new("hexDecode".into(), 1 << 5, true, 0, vec![string], bytes)
            .with_active_in(Hardfork::HfFaun),
    ]
});

impl NativeContract for StdLib {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *STDLIB_HASH_REF
    }

    fn name(&self) -> &str {
        "StdLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &STDLIB_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        dispatch(method, args).unwrap_or_else(|| {
            Err(CoreError::invalid_operation(format!(
                "StdLib method '{method}' is not implemented"
            )))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: &str, arg: &[u8]) -> CoreResult<Vec<u8>> {
        dispatch(method, &[arg.to_vec()]).expect("known method")
    }

    #[test]
    fn base64_matches_csharp() {
        // C# StdLib.Base64Encode(utf8("abc")) == "YWJj".
        assert_eq!(call("base64Encode", b"abc").unwrap(), b"YWJj");
        assert_eq!(call("base64Encode", b"").unwrap(), b"");
        assert_eq!(call("base64Encode", &[0xff, 0xfe]).unwrap(), b"//4=");
    }

    #[test]
    fn base64_decode_matches_csharp_vectors() {
        // C# UT_StdLib.TestBinary vectors (the in-repo oracle).
        // Round-trips of Base64Encode output.
        assert_eq!(call("base64Decode", b"").unwrap(), Vec::<u8>::new());
        let enc3 = call("base64Encode", &[1, 2, 3]).unwrap();
        assert_eq!(call("base64Decode", &enc3).unwrap(), vec![1, 2, 3]);
        // Whitespace {space, \r, \t, \n} is stripped before decoding.
        assert_eq!(call("base64Decode", b"A \r Q \t I \n D").unwrap(), vec![1, 2, 3]);
        assert_eq!(call("base64Decode", b"AQIDBA==").unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn base64_decode_rejects_invalid() {
        // Non-alphabet bytes fault.
        assert!(call("base64Decode", b"@@@@").is_err());
        // Whitespace other than {space, \t, \n, \r} is NOT tolerated (C# faults):
        // a vertical tab (0x0B) survives the strip and faults the strict decode.
        assert!(call("base64Decode", b"AQI\x0bD").is_err());
        // Non-multiple-of-4 length (missing padding) faults.
        assert!(call("base64Decode", b"AQI").is_err());
    }

    #[test]
    fn base64_decode_respects_max_input_length() {
        // 1024 bytes ok ("QQ==" padded chunks stay valid); 1025 faults pre-decode.
        let ok = "A".repeat(MAX_INPUT_LENGTH - 4) + "QQ==";
        assert_eq!(ok.len(), MAX_INPUT_LENGTH);
        assert!(dispatch("base64Decode", &[ok.into_bytes()]).unwrap().is_ok());
        let too_long = "A".repeat(MAX_INPUT_LENGTH + 1);
        assert!(dispatch("base64Decode", &[too_long.into_bytes()]).unwrap().is_err());
    }

    #[test]
    fn base64_url_matches_csharp_vectors() {
        // C# UT_StdLib.TestBase64Url (the in-repo oracle).
        let plain = "Subject=test@example.com&Issuer=https://example.com";
        let encoded = "U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t";
        // base64UrlEncode encodes the UTF-8 bytes of the input string.
        assert_eq!(
            String::from_utf8(call("base64UrlEncode", plain.as_bytes()).unwrap()).unwrap(),
            encoded
        );
        // base64UrlDecode returns the decoded bytes as a string.
        assert_eq!(
            String::from_utf8(call("base64UrlDecode", encoded.as_bytes()).unwrap()).unwrap(),
            plain
        );
        // The four whitespace chars .NET ignores are stripped before decoding.
        let spaced = "U 3 \t V \n \riamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t";
        assert_eq!(
            String::from_utf8(call("base64UrlDecode", spaced.as_bytes()).unwrap()).unwrap(),
            plain
        );
    }

    #[test]
    fn base64_url_decode_rejects_invalid() {
        // Standard-alphabet '+'/'/' are not URL-safe; a stray vertical tab is not
        // among the tolerated whitespace — both fault.
        assert!(call("base64UrlDecode", b"ab+/").is_err());
        assert!(call("base64UrlDecode", b"U3Vi\x0bamVjdA").is_err());
    }

    #[test]
    fn base64_url_methods_are_echidna_gated() {
        let c = StdLib::new();
        for name in ["base64UrlEncode", "base64UrlDecode"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.active_in, Some(Hardfork::HfEchidna), "{name} must gate on Echidna");
        }
    }

    #[test]
    fn hex_encode_decode_matches_csharp_vectors() {
        // C# UT_StdLib.TestHexEncodeDecode: hexEncode([0,1,2,3]) == "00010203".
        assert_eq!(call("hexEncode", &[0, 1, 2, 3]).unwrap(), b"00010203");
        assert_eq!(call("hexDecode", b"00010203").unwrap(), vec![0, 1, 2, 3]);
        assert_eq!(call("hexEncode", b"").unwrap(), b"");
        // Lowercase, no prefix; round-trips arbitrary bytes.
        assert_eq!(call("hexEncode", &[0xab, 0xff]).unwrap(), b"abff");
        assert_eq!(call("hexDecode", b"ABFF").unwrap(), vec![0xab, 0xff]); // case-insensitive
    }

    #[test]
    fn hex_decode_rejects_invalid() {
        // Odd length and non-hex characters fault (Convert.FromHexString parity).
        assert!(call("hexDecode", b"012").is_err());
        assert!(call("hexDecode", b"zz").is_err());
        assert!(call("hexDecode", b"0x10").is_err()); // no "0x" prefix accepted
    }

    #[test]
    fn hex_methods_are_faun_gated() {
        let c = StdLib::new();
        for name in ["hexEncode", "hexDecode"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.active_in, Some(Hardfork::HfFaun), "{name} must gate on Faun");
        }
    }

    #[test]
    fn base58_round_trips() {
        for sample in [&b"abc"[..], &[0u8, 0, 1, 2, 255][..], &[][..]] {
            let enc = call("base58Encode", sample).unwrap();
            assert_eq!(call("base58Decode", &enc).unwrap(), sample);

            let cenc = call("base58CheckEncode", sample).unwrap();
            assert_eq!(call("base58CheckDecode", &cenc).unwrap(), sample);
        }
        // A corrupted base58check payload must fault.
        assert!(call("base58CheckDecode", b"zzzzzzzz").is_err());
    }

    #[test]
    fn memory_compare_matches_csharp_sign() {
        let cmp = |a: &[u8], b: &[u8]| -> BigInt {
            let out = dispatch("memoryCompare", &[a.to_vec(), b.to_vec()])
                .unwrap()
                .unwrap();
            BigInt::from_signed_bytes_le(&out)
        };
        assert_eq!(cmp(b"abc", b"abc"), BigInt::from(0));
        assert_eq!(cmp(b"abc", b"abd"), BigInt::from(-1));
        assert_eq!(cmp(b"abd", b"abc"), BigInt::from(1));
        // Prefix is "less" than the longer string (SequenceCompareTo semantics).
        assert_eq!(cmp(b"ab", b"abc"), BigInt::from(-1));
        assert_eq!(cmp(b"abc", b"ab"), BigInt::from(1));
    }

    #[test]
    fn unknown_method_is_none() {
        assert!(dispatch("notAStdLibMethod", &[vec![1]]).is_none());
    }

    /// stringSplit via the dispatch seam: decodes the BinarySerialized Array
    /// return back into the substrings for comparison.
    fn split(args: &[&[u8]]) -> Vec<String> {
        let owned: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
        let bytes = dispatch("stringSplit", &owned).unwrap().unwrap();
        let item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
        item.as_array()
            .unwrap()
            .iter()
            .map(|s| String::from_utf8(s.as_bytes().unwrap()).unwrap())
            .collect()
    }

    #[test]
    fn string_split_matches_csharp_vector() {
        // C# UT_StdLib.StringSplit: stringSplit("a,b", ",") -> ["a","b"].
        assert_eq!(split(&[b"a,b", b","]), vec!["a", "b"]);
    }

    #[test]
    fn string_split_keeps_empty_entries_by_default() {
        // StringSplitOptions.None keeps empty entries (C# string.Split).
        assert_eq!(split(&[b"a,,b", b","]), vec!["a", "", "b"]);
        assert_eq!(split(&[b",a,", b","]), vec!["", "a", ""]);
        // Empty input -> a single empty element.
        assert_eq!(split(&[b"", b","]), vec![""]);
        // Multi-char separator.
        assert_eq!(split(&[b"a::b::c", b"::"]), vec!["a", "b", "c"]);
        // Empty separator -> the whole string as one element (.NET string overload).
        assert_eq!(split(&[b"abc", b""]), vec!["abc"]);
    }

    #[test]
    fn string_split_remove_empty_entries() {
        // 3-arg overload with removeEmptyEntries = true filters empties.
        assert_eq!(split(&[b"a,,b", b",", &[1]]), vec!["a", "b"]);
        assert_eq!(split(&[b",a,", b",", &[1]]), vec!["a"]);
        assert_eq!(split(&[b"", b",", &[1]]), Vec::<String>::new());
        // removeEmptyEntries = false keeps them (same as the 2-arg form).
        assert_eq!(split(&[b"a,,b", b",", &[0]]), vec!["a", "", "b"]);
    }

    /// itoa via the dispatch seam: `value` is a signed-LE Integer, optional
    /// `base` is a signed-LE Integer; the result is the UTF-8 string bytes.
    fn itoa(value: i64, base: Option<i64>) -> CoreResult<String> {
        let mut args = vec![BigInt::from(value).to_signed_bytes_le()];
        if let Some(base) = base {
            args.push(BigInt::from(base).to_signed_bytes_le());
        }
        dispatch("itoa", &args)
            .unwrap()
            .map(|b| String::from_utf8(b).unwrap())
    }

    /// atoi via the dispatch seam: `value` is UTF-8 string bytes, optional
    /// `base` is a signed-LE Integer; the result is the signed-LE Integer.
    fn atoi(value: &str, base: Option<i64>) -> CoreResult<BigInt> {
        let mut args = vec![value.as_bytes().to_vec()];
        if let Some(base) = base {
            args.push(BigInt::from(base).to_signed_bytes_le());
        }
        dispatch("atoi", &args)
            .unwrap()
            .map(|b| BigInt::from_signed_bytes_le(&b))
    }

    #[test]
    fn itoa_base10_matches_csharp() {
        // C# Itoa(value) == value.ToString().
        assert_eq!(itoa(0, None).unwrap(), "0");
        assert_eq!(itoa(123, None).unwrap(), "123");
        assert_eq!(itoa(-123, None).unwrap(), "-123");
        assert_eq!(itoa(123, Some(10)).unwrap(), "123");
    }

    #[test]
    fn itoa_base16_matches_dotnet_twos_complement() {
        // C# Itoa(value, 16) == value.ToString("x"): lowercase, sign-disambiguated.
        assert_eq!(itoa(0, Some(16)).unwrap(), "0");
        assert_eq!(itoa(1, Some(16)).unwrap(), "1");
        assert_eq!(itoa(10, Some(16)).unwrap(), "0a"); // top nibble >= 8 -> leading 0
        assert_eq!(itoa(15, Some(16)).unwrap(), "0f");
        assert_eq!(itoa(16, Some(16)).unwrap(), "10");
        assert_eq!(itoa(127, Some(16)).unwrap(), "7f");
        assert_eq!(itoa(128, Some(16)).unwrap(), "080");
        assert_eq!(itoa(255, Some(16)).unwrap(), "0ff");
        assert_eq!(itoa(256, Some(16)).unwrap(), "100");
        // Negatives render in two's complement.
        assert_eq!(itoa(-1, Some(16)).unwrap(), "f");
        assert_eq!(itoa(-16, Some(16)).unwrap(), "f0");
        assert_eq!(itoa(-128, Some(16)).unwrap(), "80");
        assert_eq!(itoa(-129, Some(16)).unwrap(), "f7f");
        assert_eq!(itoa(-256, Some(16)).unwrap(), "f00");
    }

    #[test]
    fn itoa_invalid_base_faults() {
        assert!(itoa(1, Some(2)).is_err());
        assert!(itoa(1, Some(8)).is_err());
    }

    #[test]
    fn atoi_base10_matches_csharp() {
        assert_eq!(atoi("0", None).unwrap(), BigInt::from(0));
        assert_eq!(atoi("123", None).unwrap(), BigInt::from(123));
        assert_eq!(atoi("-123", None).unwrap(), BigInt::from(-123));
        assert_eq!(atoi("+123", None).unwrap(), BigInt::from(123));
        assert_eq!(atoi("-0", None).unwrap(), BigInt::from(0));
        // AllowLeadingSign rejects whitespace, separators, and junk.
        assert!(atoi(" 1", None).is_err());
        assert!(atoi("1 ", None).is_err());
        assert!(atoi("1.0", None).is_err());
        assert!(atoi("", None).is_err());
        assert!(atoi("+", None).is_err());
        assert!(atoi("0x10", None).is_err());
    }

    #[test]
    fn atoi_base16_matches_dotnet_twos_complement() {
        // AllowHexSpecifier: leading nibble >= 8 -> negative.
        assert_eq!(atoi("ff", Some(16)).unwrap(), BigInt::from(-1));
        assert_eq!(atoi("0ff", Some(16)).unwrap(), BigInt::from(255));
        assert_eq!(atoi("f", Some(16)).unwrap(), BigInt::from(-1));
        assert_eq!(atoi("0f", Some(16)).unwrap(), BigInt::from(15));
        assert_eq!(atoi("80", Some(16)).unwrap(), BigInt::from(-128));
        assert_eq!(atoi("080", Some(16)).unwrap(), BigInt::from(128));
        assert_eq!(atoi("7f", Some(16)).unwrap(), BigInt::from(127));
        assert_eq!(atoi("100", Some(16)).unwrap(), BigInt::from(256));
        assert_eq!(atoi("f00", Some(16)).unwrap(), BigInt::from(-256));
        // Case-insensitive; a leading sign is NOT allowed for hex.
        assert_eq!(atoi("FF", Some(16)).unwrap(), BigInt::from(-1));
        assert!(atoi("-1", Some(16)).is_err());
        assert!(atoi("zz", Some(16)).is_err());
    }

    #[test]
    fn itoa_atoi_round_trip_hex() {
        // atoi(itoa(v, 16), 16) == v across the sign boundary.
        for v in [-300i64, -256, -129, -128, -1, 0, 1, 127, 128, 255, 256, 65535] {
            let hex = itoa(v, Some(16)).unwrap();
            assert_eq!(atoi(&hex, Some(16)).unwrap(), BigInt::from(v), "hex={hex}");
        }
    }

    #[test]
    fn atoi_respects_max_input_length() {
        // C# [MaxLength(1024)] on the input: 1024 bytes ok, 1025 faults.
        let ok = "1".repeat(MAX_INPUT_LENGTH);
        assert!(dispatch("atoi", &[ok.into_bytes()]).unwrap().is_ok());
        let too_long = "1".repeat(MAX_INPUT_LENGTH + 1);
        assert!(dispatch("atoi", &[too_long.into_bytes()]).unwrap().is_err());
    }

    #[test]
    fn native_contract_surface() {
        let c = StdLib::new();
        assert_eq!(NativeContract::id(&c), -2);
        assert_eq!(NativeContract::name(&c), "StdLib");
        assert_eq!(NativeContract::hash(&c), *STDLIB_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "base64Encode",
                "base64Decode",
                "base58Encode",
                "base58Decode",
                "base58CheckEncode",
                "base58CheckDecode",
                "serialize",
                "deserialize",
                "jsonSerialize",
                "jsonDeserialize",
                "memoryCompare",
                "memorySearch",
                "memorySearch",
                "memorySearch",
                "itoa",
                "itoa",
                "atoi",
                "atoi",
                "stringSplit",
                "stringSplit",
                "strLen",
                "base64UrlEncode",
                "base64UrlDecode",
                "hexEncode",
                "hexDecode"
            ]
        );
        assert!(c.methods().iter().all(|m| m.safe));
        // The three memorySearch overloads are distinguished by parameter count.
        let counts: Vec<usize> = c
            .methods()
            .iter()
            .filter(|m| m.name == "memorySearch")
            .map(|m| m.parameters.len())
            .collect();
        assert_eq!(counts, [2, 3, 4]);
    }

    /// strLen via the dispatch seam: UTF-8 string bytes in, signed-LE Integer out.
    fn str_len(arg: &[u8]) -> CoreResult<i64> {
        dispatch("strLen", &[arg.to_vec()])
            .unwrap()
            .map(|b| BigInt::from_signed_bytes_le(&b).to_i64().unwrap())
    }

    #[test]
    fn str_len_matches_csharp_ut_vectors() {
        // C# UT_StdLib.StringElementLength: duck emoji, a-tilde and 'a' are all 1.
        assert_eq!(str_len("\u{1F986}".as_bytes()).unwrap(), 1);
        assert_eq!(str_len("\u{00E3}".as_bytes()).unwrap(), 1);
        assert_eq!(str_len(b"a").unwrap(), 1);
        // C# UT_StdLib.TestInvalidUtf8Sequence: (char)0xff is emitted as the
        // UTF-8 encoding of U+00FF (C3 BF) and counts as one element.
        assert_eq!(str_len(&[0xC3, 0xBF]).unwrap(), 1);
        assert_eq!(str_len(&[0xC3, 0xBF, b'a', b'b']).unwrap(), 3);
        // Decomposed a-tilde is also a single element; empty string is 0.
        assert_eq!(str_len("a\u{0303}".as_bytes()).unwrap(), 1);
        assert_eq!(str_len(b"").unwrap(), 0);
        // The .NET-specific divergence: no GB9c, Indic conjuncts stay split.
        assert_eq!(str_len("\u{0915}\u{094D}\u{0915}".as_bytes()).unwrap(), 2);
        // Emoji ZWJ family sequence and a flag are one element each.
        assert_eq!(
            str_len("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}".as_bytes())
                .unwrap(),
            1
        );
        assert_eq!(str_len("\u{1F1FA}\u{1F1F8}".as_bytes()).unwrap(), 1);
    }

    #[test]
    fn str_len_rejects_invalid_utf8() {
        // C# converts the ByteString with StrictUTF8: invalid UTF-8 faults.
        assert!(str_len(&[0xFF]).is_err());
        assert!(str_len(&[0xC3]).is_err()); // truncated sequence
        assert!(str_len(&[0xED, 0xA0, 0x80]).is_err()); // surrogate encoding
    }

    #[test]
    fn str_len_respects_max_input_length() {
        // C# [MaxLength(1024)] validates the raw StackItem bytes.
        let ok = vec![b'a'; MAX_INPUT_LENGTH];
        assert_eq!(str_len(&ok).unwrap(), 1024);
        let too_long = vec![b'a'; MAX_INPUT_LENGTH + 1];
        assert!(str_len(&too_long).is_err());
        // The cap is on bytes, not characters: 342 three-byte scalars = 1026 bytes.
        let multibyte = "\u{20AC}".repeat(342);
        assert_eq!(multibyte.len(), 1026);
        assert!(str_len(multibyte.as_bytes()).is_err());
    }

    #[test]
    fn str_len_is_ungated_and_safe() {
        // C# StdLib.cs declares StrLen with CpuFee = 1 << 8 and no hardfork.
        let c = StdLib::new();
        let m = c.methods().iter().find(|m| m.name == "strLen").unwrap();
        assert_eq!(m.active_in, None, "strLen must not be hardfork-gated");
        assert!(m.safe);
        assert_eq!(m.cpu_fee, 1 << 8);
        assert_eq!(m.parameters, vec![ContractParameterType::String]);
        assert_eq!(m.return_type, ContractParameterType::Integer);
    }

    #[test]
    fn memory_search_matches_csharp() {
        let search = |args: &[&[u8]]| -> i64 {
            let owned: Vec<Vec<u8>> = args.iter().map(|a| a.to_vec()).collect();
            let out = dispatch("memorySearch", &owned).unwrap().unwrap();
            BigInt::from_signed_bytes_le(&out).to_i64().unwrap()
        };
        let n = |v: i64| BigInt::from(v).to_signed_bytes_le();

        // Forward (2-arg): first occurrence, or -1.
        assert_eq!(search(&[b"hello world", b"o"]), 4);
        assert_eq!(search(&[b"hello world", b"world"]), 6);
        assert_eq!(search(&[b"hello", b"z"]), -1);
        // 3-arg: start offset is added back to the in-slice index.
        assert_eq!(search(&[b"hello world", b"o", &n(5)]), 7);
        // 4-arg backward: last occurrence within mem[0..start].
        assert_eq!(search(&[b"hello world", b"o", &n(11), &[1]]), 7);
        assert_eq!(search(&[b"hello world", b"o", &n(5), &[1]]), 4);
    }

    #[test]
    fn memory_search_start_out_of_range_faults() {
        // C# AsSpan(start) throws when start exceeds the length.
        assert!(dispatch("memorySearch", &[b"abc".to_vec(), b"a".to_vec(), vec![9]])
            .unwrap()
            .is_err());
    }

    #[test]
    fn serialize_deserialize_round_trip_and_fault() {
        use neo_vm::StackItem;
        // The serialize arg arrives already BinarySerialized by the engine, so
        // dispatch("serialize") is a passthrough of that payload.
        let payload = BinarySerializer::serialize(
            &StackItem::from_int(BigInt::from(42)),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        assert_eq!(
            dispatch("serialize", &[payload.clone()]).unwrap().unwrap(),
            payload
        );
        // deserialize accepts the valid payload (returns it for the Any-return
        // decode) and faults on malformed input.
        assert_eq!(
            dispatch("deserialize", &[payload.clone()]).unwrap().unwrap(),
            payload
        );
        assert!(dispatch("deserialize", &[vec![0xff, 0xff, 0xff]])
            .unwrap()
            .is_err());
    }

    #[test]
    fn json_serialize_deserialize_match_csharp_vectors() {
        use neo_vm::StackItem;
        let limits = ExecutionEngineLimits::default();
        // The engine BinarySerializes the `Any` arg before dispatch sees it.
        let ser = |item: &StackItem| -> String {
            let payload = BinarySerializer::serialize(item, &limits).unwrap();
            let json = dispatch("jsonSerialize", &[payload]).unwrap().unwrap();
            String::from_utf8(json).unwrap()
        };
        // C# UT_StdLib.Json_Serialize.
        assert_eq!(ser(&StackItem::from_int(BigInt::from(5))), "5");
        assert_eq!(ser(&StackItem::from_bool(true)), "true");
        assert_eq!(ser(&StackItem::from_byte_string(b"test".to_vec())), "\"test\"");
        assert_eq!(ser(&StackItem::null()), "null");

        // jsonDeserialize returns the StackItem re-encoded as BinarySerializer
        // bytes (for the engine's Any-return decode); compare to the direct
        // encoding of the expected item.
        let de_eq = |json: &str, item: &StackItem| {
            let out = dispatch("jsonDeserialize", &[json.as_bytes().to_vec()])
                .unwrap()
                .unwrap();
            assert_eq!(out, BinarySerializer::serialize(item, &limits).unwrap(), "{json}");
        };
        // C# UT_StdLib.Json_Deserialize.
        de_eq("123", &StackItem::from_int(BigInt::from(123)));
        de_eq("null", &StackItem::null());
        // Faults: invalid JSON ("***") and a fractional number ("no decimals").
        assert!(dispatch("jsonDeserialize", &[b"***".to_vec()]).unwrap().is_err());
        assert!(dispatch("jsonDeserialize", &[b"123.45".to_vec()]).unwrap().is_err());

        // Serialize -> deserialize round-trips a structured value.
        let payload = dispatch("jsonDeserialize", &[br#"{"k":[1,true,null]}"#.to_vec()])
            .unwrap()
            .unwrap();
        let item = BinarySerializer::deserialize(&payload, &limits, None).unwrap();
        assert_eq!(ser(&item), r#"{"k":[1,true,null]}"#);
    }
}

//! # neo-native-contracts::std_lib
//!
//! Native StdLib string, memory, and serialization helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `encoding`: encoding and decoding routines.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `serialization`: serialization codecs and compatibility checks.
//! - `tests`: Module-local tests and regression coverage.

mod encoding;
mod metadata;
mod serialization;

use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_serialization::BinarySerializer;
use neo_vm_rs::StackValue;
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

use crate::hashes::STDLIB_HASH;
use crate::text::dotnet_text_segmentation::text_element_count;

/// C# `StdLib.MaxInputLength` — the `[MaxLength]` cap on string/byte inputs.
const MAX_INPUT_LENGTH: usize = 1024;

native_contract_handle!(
    /// The StdLib native contract.
    pub struct StdLib {
        id: -2,
        contract_name: "StdLib",
        hash: STDLIB_HASH,
    }
);

impl StdLib {
    fn arg_bytes<'a>(args: &'a [Vec<u8>], method: &str) -> CoreResult<&'a [u8]> {
        args.first().map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation(format!("StdLib::{method} requires one argument"))
        })
    }

    fn ensure_max_len(method: &str, param: &str, value: &[u8]) -> CoreResult<()> {
        if value.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::{method}: {param} exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        Ok(())
    }

    fn arg_bytes_max<'a>(args: &'a [Vec<u8>], method: &str, param: &str) -> CoreResult<&'a [u8]> {
        let value = Self::arg_bytes(args, method)?;
        Self::ensure_max_len(method, param, value)?;
        Ok(value)
    }

    fn arg_str_max(args: &[Vec<u8>], method: &str, param: &str) -> CoreResult<String> {
        let value = Self::arg_bytes_max(args, method, param)?;
        String::from_utf8(value.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }

    /// Interprets the single argument as a native string (a VM ByteString carrying
    /// UTF-8 bytes).
    fn arg_str(args: &[Vec<u8>], method: &str) -> CoreResult<String> {
        String::from_utf8(Self::arg_bytes(args, method)?.to_vec()).map_err(|_| {
            CoreError::invalid_operation(format!("StdLib::{method}: argument is not valid UTF-8"))
        })
    }

    /// Pure dispatch for StdLib's stateless methods, split out so it can be unit
    /// tested without constructing an [`ApplicationEngine`]. Returns `None` for an
    /// unknown method so [`StdLib::invoke`] can emit a precise error.
    ///
    /// `basilisk_active` is the only piece of block-height context StdLib needs: it
    /// is `engine.IsHardforkEnabled(Hardfork.HF_Basilisk)` and gates only
    /// `jsonDeserialize`'s number handling (every other method is height-independent).
    /// [`StdLib::invoke`] supplies it from the engine; unit tests pass the era under
    /// test directly.
    fn dispatch(
        method: &str,
        args: &[Vec<u8>],
        basilisk_active: bool,
    ) -> Option<CoreResult<Vec<u8>>> {
        let result = match method {
            // Encoders: ByteArray -> String (returned as UTF-8 bytes).
            "base64Encode" => encoding::base64_encode_impl(args),
            // base64Decode: String -> ByteArray (C# Convert.FromBase64String).
            "base64Decode" => encoding::base64_decode_impl(args),
            // base64Url* (HF_Echidna): String <-> String, URL-safe alphabet, no padding.
            "base64UrlEncode" => encoding::base64_url_encode_impl(args),
            "base64UrlDecode" => encoding::base64_url_decode_impl(args),
            // hexEncode/hexDecode (HF_Faun): ByteArray <-> lowercase hex String.
            "hexEncode" => encoding::hex_encode_impl(args),
            "hexDecode" => encoding::hex_decode_impl(args),
            "base58Encode" => encoding::base58_encode_impl(args),
            "base58CheckEncode" => encoding::base58_check_encode_impl(args),
            // Decoders: String -> ByteArray. Invalid input faults the call, matching
            // C# (Base58 throws on a bad alphabet / checksum).
            "base58Decode" => encoding::base58_decode_impl(args),
            "base58CheckDecode" => encoding::base58_check_decode_impl(args),
            // memoryCompare(a, b) -> Math.Sign(a.SequenceCompareTo(b)) as Integer.
            // Rust slice `cmp` is the same lexicographic-then-length ordering.
            "memoryCompare" => match (args.first(), args.get(1)) {
                (Some(a), Some(b)) => {
                    match (
                        Self::ensure_max_len(method, "str1", a),
                        Self::ensure_max_len(method, "str2", b),
                    ) {
                        (Err(e), _) | (_, Err(e)) => Err(e),
                        (Ok(()), Ok(())) => {
                            let sign: i32 = match a.as_slice().cmp(b.as_slice()) {
                                std::cmp::Ordering::Less => -1,
                                std::cmp::Ordering::Equal => 0,
                                std::cmp::Ordering::Greater => 1,
                            };
                            Ok(BigInt::from(sign).to_signed_bytes_le())
                        }
                    }
                }
                _ => Err(CoreError::invalid_operation(
                    "StdLib::memoryCompare requires two arguments".to_string(),
                )),
            },
            // memorySearch(mem, value[, start[, backward]]) -> Integer index or -1.
            "memorySearch" => Self::memory_search_impl(args),
            // itoa(value[, base]) -> String; atoi(value[, base]) -> Integer.
            "itoa" => Self::itoa_impl(args),
            "atoi" => Self::atoi_impl(args),
            // stringSplit(str, separator[, removeEmptyEntries]) -> Array of String.
            "stringSplit" => Self::string_split_impl(args),
            // strLen(str) -> Integer: the .NET StringInfo text-element count.
            "strLen" => Self::str_len_impl(args),
            "serialize" => serialization::serialize_impl(args),
            "deserialize" => serialization::deserialize_impl(args),
            "jsonSerialize" => serialization::json_serialize_impl(args),
            "jsonDeserialize" => serialization::json_deserialize_impl(args, basilisk_active),
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
        Self::ensure_max_len("memorySearch", "mem", mem)?;
        let value = args.get(1).map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation("StdLib::memorySearch requires (mem, value)")
        })?;
        // C# marshals the `int start` parameter with `(int)p.GetInteger()`, a
        // TRUNCATING two's-complement cast to the low 32 bits (wrapping, not
        // faulting on out-of-range). `MemorySearch` then does `AsSpan(start)` /
        // `AsSpan(0, start)`, which throw only for `start < 0` or `start > length`.
        let start_i32 = match args.get(2) {
            Some(b) => Self::dotnet_int_cast(&BigInt::from_signed_bytes_le(b)),
            None => 0,
        };
        if start_i32 < 0 || i64::from(start_i32) > mem.len() as i64 {
            return Err(CoreError::invalid_operation(
                "StdLib::memorySearch: start out of range",
            ));
        }
        let start = start_i32 as usize;
        let backward = args
            .get(3)
            .map(|b| b.iter().any(|x| *x != 0))
            .unwrap_or(false);
        Ok(BigInt::from(Self::memory_search(mem, value, start, backward)).to_signed_bytes_le())
    }

    fn memory_search(mem: &[u8], value: &[u8], start: usize, backward: bool) -> i64 {
        if backward {
            Self::last_index_of(&mem[..start], value)
        } else {
            match Self::index_of(&mem[start..], value) {
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

    /// Emulates the .NET `(int)BigInteger` narrowing cast that
    /// `InteropParameterDescriptor` applies to every `int`-typed native parameter
    /// (`[typeof(int)] = p => (int)p.GetInteger()`): the low 32 bits reinterpreted
    /// as a two's-complement `i32`. It WRAPS and never faults — so an out-of-`i32`
    /// argument (e.g. `2^32 + 10`) becomes a small in-range value (`10`), which is
    /// then validated by the method itself. Using `to_i32()`/`to_usize()` instead
    /// would fault where C# succeeds, forking any contract that passes such a value.
    fn dotnet_int_cast(value: &BigInt) -> i32 {
        (value & BigInt::from(0xFFFF_FFFFu32)).to_u32().unwrap_or(0) as i32
    }

    /// Reads an optional integer `base` argument (C# StdLib's `int @base` overload),
    /// defaulting to 10 when absent. C# marshals it with the truncating `(int)`
    /// cast; the caller then rejects any base other than 10 or 16.
    fn optional_base(args: &[Vec<u8>], index: usize, _method: &str) -> CoreResult<i64> {
        match args.get(index) {
            None => Ok(10),
            Some(bytes) => Ok(i64::from(Self::dotnet_int_cast(
                &BigInt::from_signed_bytes_le(bytes),
            ))),
        }
    }

    /// C# `StdLib.Itoa(value[, base])`: base 10 -> `BigInteger.ToString()` (decimal),
    /// base 16 -> `BigInteger.ToString("x")` (lowercase two's-complement hex).
    /// Any other base throws `ArgumentOutOfRangeException`.
    fn itoa_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let value = BigInt::from_signed_bytes_le(Self::arg_bytes(args, "itoa")?);
        let text = match Self::optional_base(args, 1, "itoa")? {
            10 => value.to_str_radix(10),
            16 => Self::dotnet_bigint_to_hex(&value),
            other => {
                return Err(CoreError::invalid_argument(format!(
                    "StdLib::itoa: invalid base: {other}"
                )));
            }
        };
        Ok(text.into_bytes())
    }

    /// C# `StdLib.Atoi(value[, base])`: base 10 -> `BigInteger.Parse(AllowLeadingSign)`,
    /// base 16 -> `BigInteger.Parse(AllowHexSpecifier)` (two's-complement). Enforces
    /// the C# `[MaxLength(1024)]` cap on the input. Any other base throws.
    fn atoi_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let raw = Self::arg_bytes(args, "atoi")?;
        if raw.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::atoi: input exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        let value = std::str::from_utf8(raw).map_err(|_| {
            CoreError::invalid_operation("StdLib::atoi: argument is not valid UTF-8".to_string())
        })?;
        let parsed = match Self::optional_base(args, 1, "atoi")? {
            10 => Self::parse_dotnet_decimal(value)?,
            16 => Self::parse_dotnet_hex(value)?,
            other => {
                return Err(CoreError::invalid_argument(format!(
                    "StdLib::atoi: invalid base: {other}"
                )));
            }
        };
        Ok(parsed.to_signed_bytes_le())
    }

    /// C# `StdLib.StringSplit(str, separator[, removeEmptyEntries])` = `String.Split`:
    /// split `str` on each occurrence of `separator`, keeping empty entries unless
    /// `removeEmptyEntries` is true. An empty separator yields `[str]` (the whole
    /// string), matching .NET's `string.Split(string)` overload. Enforces the C#
    /// `[MaxLength(1024)]` cap on `str`. Returns a VM Array of ByteStrings
    /// (BinarySerialized; the engine deserializes it for the `Array` return type).
    fn string_split_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let raw = Self::arg_bytes(args, "stringSplit")?;
        if raw.len() > MAX_INPUT_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "StdLib::stringSplit: input exceeds maximum length ({MAX_INPUT_LENGTH})"
            )));
        }
        let value = std::str::from_utf8(raw).map_err(|_| {
            CoreError::invalid_operation(
                "StdLib::stringSplit: argument is not valid UTF-8".to_string(),
            )
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
                ));
            }
        };
        let remove_empty = args
            .get(2)
            .map(|b| b.iter().any(|x| *x != 0))
            .unwrap_or(false);

        let parts: Vec<&str> = if separator.is_empty() {
            // .NET `string.Split("")` returns the whole string as a single element.
            vec![value]
        } else {
            value.split(separator).collect()
        };
        let items: Vec<StackValue> = parts
            .into_iter()
            .filter(|part| !remove_empty || !part.is_empty())
            .map(|part| StackValue::ByteString(part.as_bytes().to_vec()))
            .collect();

        BinarySerializer::serialize_stack_value_default(&StackValue::Array(items))
            .map_err(|e| CoreError::invalid_operation(format!("StdLib::stringSplit: {e}")))
    }

    /// C# `StdLib.StrLen(str)`: the number of text elements in the string, i.e.
    /// .NET `StringInfo` extended grapheme clusters (UAX #29 minus GB9c over the
    /// .NET runtime's break-property snapshot; see
    /// [`crate::dotnet_text_segmentation`]). Enforces the C# `[MaxLength(1024)]`
    /// cap on the raw input bytes; invalid UTF-8 faults the call, matching the C#
    /// `StrictUTF8` string conversion.
    fn str_len_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        let raw = Self::arg_bytes(args, "strLen")?;
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
}

impl NativeContract for StdLib {
    native_contract_identity!(StdLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::STD_LIB_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // The only block-height-dependent StdLib behavior is jsonDeserialize's
        // number handling, gated on HF_Basilisk (C# JsonSerializer.Deserialize).
        let basilisk_active = engine.is_hardfork_enabled(Hardfork::HfBasilisk);
        Self::dispatch(method, args, basilisk_active).unwrap_or_else(|| {
            Err(CoreError::invalid_operation(format!(
                "StdLib method '{method}' is not implemented"
            )))
        })
    }
}

#[cfg(test)]
#[path = "../tests/std_lib/mod.rs"]
mod tests;

//! StdLib integer formatting and parsing helpers.
//!
//! These helpers preserve C# BigInteger formatting, parsing, and native
//! `int` narrowing semantics for itoa/atoi and memorySearch overloads.

use super::{StdLib, args::MAX_INPUT_LENGTH};
use neo_error::{CoreError, CoreResult};
use num_bigint::{BigInt, Sign};
use num_traits::ToPrimitive;

impl StdLib {
    /// Emulates the .NET `(int)BigInteger` narrowing cast that
    /// `InteropParameterDescriptor` applies to every `int`-typed native parameter
    /// (`[typeof(int)] = p => (int)p.GetInteger()`): the low 32 bits
    /// reinterpreted as a two's-complement `i32`. It WRAPS and never faults -
    /// so an out-of-`i32` argument (e.g. `2^32 + 10`) becomes a small in-range
    /// value (`10`), which is then validated by the method itself. Using
    /// `to_i32()`/`to_usize()` instead would fault where C# succeeds, forking
    /// any contract that passes such a value.
    pub(super) fn dotnet_int_cast(value: &BigInt) -> i32 {
        (value & BigInt::from(0xFFFF_FFFFu32)).to_u32().unwrap_or(0) as i32
    }

    /// Reads an optional integer `base` argument (C# StdLib's `int @base`
    /// overload), defaulting to 10 when absent. C# marshals it with the
    /// truncating `(int)` cast; the caller then rejects any base other than 10
    /// or 16.
    fn optional_base(args: &[Vec<u8>], index: usize, _method: &str) -> CoreResult<i64> {
        match args.get(index) {
            None => Ok(10),
            Some(bytes) => Ok(i64::from(Self::dotnet_int_cast(
                &BigInt::from_signed_bytes_le(bytes),
            ))),
        }
    }

    /// C# `StdLib.Itoa(value[, base])`: base 10 ->
    /// `BigInteger.ToString(CultureInfo.InvariantCulture)`, base 16 ->
    /// `BigInteger.ToString("x", CultureInfo.InvariantCulture)` (lowercase
    /// two's-complement hex). Rust formatting is culture-invariant. Any other
    /// base throws `ArgumentOutOfRangeException`.
    pub(super) fn itoa_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
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

    /// C# `StdLib.Atoi(value[, base])`: base 10 ->
    /// `BigInteger.Parse(AllowLeadingSign)`, base 16 ->
    /// `BigInteger.Parse(AllowHexSpecifier)` (two's-complement). Enforces the
    /// C# `[MaxLength(1024)]` cap on the input. Any other base throws.
    pub(super) fn atoi_impl(args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
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

    /// Mirrors .NET `BigInteger.ToString("x")`: lowercase, minimal
    /// two's-complement hex with a sign-disambiguating leading nibble (a
    /// positive value whose top nibble is >= 8 gets a leading `0`; negatives
    /// are rendered in two's complement, e.g. `-1` -> "f", `255` -> "0ff",
    /// `-256` -> "f00").
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

    /// Mirrors .NET `BigInteger.Parse(value, NumberStyles.AllowLeadingSign)`:
    /// an optional leading `+`/`-` then one or more decimal digits, nothing else
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

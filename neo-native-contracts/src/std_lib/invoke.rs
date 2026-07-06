//! StdLib native-method dispatch.
//!
//! Keeps the method-name routing and hardfork-gated invoke wrapper separate
//! from the string, memory, and serialization implementations.

use super::{StdLib, encoding, serialization};
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use num_bigint::BigInt;

impl StdLib {
    /// Pure dispatch for StdLib's stateless methods, split out so it can be unit
    /// tested without constructing an [`ApplicationEngine`]. Returns `None` for an
    /// unknown method so [`StdLib::invoke_native`] can emit a precise error.
    ///
    /// `basilisk_active` is the only piece of block-height context StdLib needs: it
    /// is `engine.IsHardforkEnabled(Hardfork.HF_Basilisk)` and gates only
    /// `jsonDeserialize`'s number handling (every other method is height-independent).
    /// [`StdLib::invoke_native`] supplies it from the engine; unit tests pass the era
    /// under test directly.
    pub(in crate::std_lib) fn dispatch(
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

    pub(super) fn invoke_native(
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

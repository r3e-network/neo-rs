//! StdLib native contract (id -2).
//!
//! Implements the C# `Neo.SmartContract.Native.StdLib` Base64/Base58 primitives
//! plus `memoryCompare` / `memorySearch`, dispatched through the
//! [`NativeContract`] trait. The remaining StdLib surface (`base64Decode` —
//! pending a strict, whitespace-exact decoder to match `Convert.FromBase64String`;
//! `itoa`/`atoi` — .NET two's-complement hex semantics; `jsonSerialize`/
//! `jsonDeserialize`, `serialize`/`deserialize`, `stringSplit`, `strLen`
//! — grapheme counting, `base64Url*`) is the next increment; every method
//! declared below is byte-for-byte C# parity with a real implementation.

use std::any::Any;
use std::sync::LazyLock;

use neo_crypto::{Base58, Base64};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::STDLIB_HASH;

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

static STDLIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let bytes = ContractParameterType::ByteArray;
    let string = ContractParameterType::String;
    let int = ContractParameterType::Integer;
    let boolean = ContractParameterType::Boolean;
    vec![
        NativeMethod::new("base64Encode".into(), 1 << 5, true, 0, vec![bytes], string),
        NativeMethod::new("base58Encode".into(), 1 << 13, true, 0, vec![bytes], string),
        NativeMethod::new("base58Decode".into(), 1 << 10, true, 0, vec![string], bytes),
        NativeMethod::new("base58CheckEncode".into(), 1 << 16, true, 0, vec![bytes], string),
        NativeMethod::new("base58CheckDecode".into(), 1 << 16, true, 0, vec![string], bytes),
        // serialize(Any) -> ByteArray; deserialize(ByteArray) -> Any.
        NativeMethod::new("serialize".into(), 1 << 12, true, 0, vec![ContractParameterType::Any], bytes),
        NativeMethod::new("deserialize".into(), 1 << 14, true, 0, vec![bytes], ContractParameterType::Any),
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
        assert!(dispatch("itoa", &[vec![1]]).is_none());
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
                "base58Encode",
                "base58Decode",
                "base58CheckEncode",
                "base58CheckDecode",
                "serialize",
                "deserialize",
                "memoryCompare",
                "memorySearch",
                "memorySearch",
                "memorySearch"
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
}

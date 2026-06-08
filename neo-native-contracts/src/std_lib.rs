//! StdLib native contract (id -2).
//!
//! Implements the C# `Neo.SmartContract.Native.StdLib` Base64-encode / Base58
//! primitives, dispatched through the [`NativeContract`] trait. The remaining
//! StdLib surface (`base64Decode` — pending a strict, whitespace-exact decoder
//! to match `Convert.FromBase64String`; `itoa`/`atoi`, `jsonSerialize`/
//! `jsonDeserialize`, `serialize`/`deserialize`, `memoryCompare`/`memorySearch`,
//! `stringSplit`, `strLen`, `base64Url*`) is the next increment; every method
//! declared below is byte-for-byte C# parity with a real implementation.

use std::any::Any;
use std::sync::LazyLock;

use neo_crypto::{Base58, Base64};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use num_bigint::BigInt;

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
        _ => return None,
    };
    Some(result)
}

static STDLIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let bytes = ContractParameterType::ByteArray;
    let string = ContractParameterType::String;
    vec![
        NativeMethod::new("base64Encode".into(), 1 << 5, true, 0, vec![bytes], string),
        NativeMethod::new("base58Encode".into(), 1 << 13, true, 0, vec![bytes], string),
        NativeMethod::new("base58Decode".into(), 1 << 10, true, 0, vec![string], bytes),
        NativeMethod::new("base58CheckEncode".into(), 1 << 16, true, 0, vec![bytes], string),
        NativeMethod::new("base58CheckDecode".into(), 1 << 16, true, 0, vec![string], bytes),
        NativeMethod::new(
            "memoryCompare".into(),
            1 << 5,
            true,
            0,
            vec![bytes, bytes],
            ContractParameterType::Integer,
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
                "memoryCompare"
            ]
        );
        assert!(c.methods().iter().all(|m| m.safe));
    }
}

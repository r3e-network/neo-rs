//! CryptoLib native contract (id -3).
//!
//! Implements the C# `Neo.SmartContract.Native.CryptoLib` hash primitives
//! (`sha256`, `ripemd160`, `keccak256`) with byte-for-byte parity, dispatched
//! through the [`NativeContract`] trait so the application engine can invoke
//! them. The remaining CryptoLib surface (`murmur32`, `verifyWithECDsa`,
//! `verifyWithEd25519`, `recoverSecp256K1`, and the BLS12-381 operations) is
//! the next increment; the methods declared in [`CryptoLib::methods`] all have
//! working implementations — none are stubs.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::{murmur32, Crypto, HashAlgorithm, NamedCurveHash, Secp256k1Crypto};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::CRYPTO_LIB_HASH;

/// Lazily-initialised script-hash handle for the CryptoLib contract.
pub static CRYPTO_LIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *CRYPTO_LIB_HASH);

/// The CryptoLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct CryptoLib;

impl CryptoLib {
    /// Stable native contract id (matches C# `CryptoLib`).
    pub const ID: i32 = -3;

    /// Construct a new `CryptoLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the CryptoLib script hash.
    pub fn script_hash() -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }
}

// C# `CpuFee = 1 << 15` for sha256 / ripemd160 / keccak256.
const CPU_FEE_HASH: i64 = 1 << 15;

/// Computes a CryptoLib hash method, returning `None` for an unknown method.
///
/// Split out from [`CryptoLib::invoke`] so the dispatch + hashing can be unit
/// tested without constructing an [`ApplicationEngine`].
fn hash_method(method: &str, data: &[u8]) -> Option<Vec<u8>> {
    match method {
        "sha256" => Some(Crypto::sha256(data).to_vec()),
        "ripemd160" => Some(Crypto::ripemd160(data).to_vec()),
        "keccak256" => Some(Crypto::keccak256(data).to_vec()),
        _ => None,
    }
}

/// Pure Ed25519 verification with C# `VerifyWithEd25519` semantics: a wrong-length
/// signature (64) or public key (32), or any verification error, yields `false`.
/// Split out so it can be unit tested without an [`ApplicationEngine`].
fn verify_ed25519_method(message: &[u8], pubkey: &[u8], signature: &[u8]) -> bool {
    signature.len() == 64
        && pubkey.len() == 32
        && neo_crypto::ecc::verify_ed25519(pubkey, message, signature).unwrap_or(false)
}

/// Pure ECDSA verification with C# `VerifyWithECDsa` semantics, split out so the
/// curve/hash dispatch can be unit tested without an [`ApplicationEngine`].
///
/// `allow_keccak` reflects the `HF_Cockatrice` hardfork (the V0/V1 split): before
/// Cockatrice only the SHA-256 named curves are valid, so a Keccak-256 curve
/// faults (C# `VerifyWithECDsaV0` throws `ArgumentOutOfRangeException`); an
/// undefined `curve` byte also faults (C# `s_curves[...]` `KeyNotFoundException`).
/// A malformed key or signature yields `Ok(false)` (the C# `ArgumentException`
/// catch).
fn verify_ecdsa_method(
    message: &[u8],
    pubkey: &[u8],
    signature: &[u8],
    curve: u8,
    allow_keccak: bool,
) -> CoreResult<bool> {
    let named = NamedCurveHash::from_byte(curve).ok_or_else(|| {
        CoreError::invalid_operation(format!("CryptoLib::verifyWithECDsa: unsupported curve {curve}"))
    })?;
    if !allow_keccak && matches!(named.hash_algorithm(), HashAlgorithm::Keccak256) {
        return Err(CoreError::invalid_operation(
            "CryptoLib::verifyWithECDsa: Keccak256 curves require the Cockatrice hardfork",
        ));
    }
    Ok(neo_crypto::ecc::verify_signature_with_hash(
        named.curve(),
        pubkey,
        message,
        signature,
        named.hash_algorithm(),
    )
    .unwrap_or(false))
}

/// Pure secp256k1 public-key recovery with C# `RecoverSecp256K1` semantics:
/// returns the 33-byte compressed public key, or `None` when recovery fails (C#
/// wraps `Crypto.ECRecover` in try/catch and returns `null`). Split out so the
/// success/null decision can be unit tested without an [`ApplicationEngine`].
fn recover_secp256k1_method(message_hash: &[u8], signature: &[u8]) -> Option<Vec<u8>> {
    Secp256k1Crypto::recover_public_key(message_hash, signature).ok()
}

static CRYPTO_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let byte_array = ContractParameterType::ByteArray;
    vec![
        // Unconditional since genesis.
        NativeMethod::new(
            "sha256".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        ),
        NativeMethod::new(
            "ripemd160".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        ),
        // Activated by the Cockatrice hardfork
        // (C# `[ContractMethod(Hardfork.HF_Cockatrice, ...)]`).
        NativeMethod::new(
            "keccak256".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_active_in(Hardfork::HfCockatrice),
        // murmur32(data: ByteArray, seed: Integer) -> ByteArray, C# CpuFee 1<<13.
        NativeMethod::new(
            "murmur32".to_string(),
            1 << 13,
            true,
            0,
            vec![byte_array, ContractParameterType::Integer],
            byte_array,
        ),
        // verifyWithEd25519(message, pubkey, signature) -> bool (HF_Echidna).
        NativeMethod::new(
            "verifyWithEd25519".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array, byte_array],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna),
        // verifyWithECDsa(message, pubkey, signature, curveHash) -> bool. Present
        // from genesis (C# V0, SHA-256 curves only); HF_Cockatrice (V1) adds the
        // Keccak-256 curves. The ABI signature is identical across versions, so a
        // single always-active registration is manifest-equivalent and the Keccak
        // gate is applied in invoke via the HF_Cockatrice check.
        NativeMethod::new(
            "verifyWithECDsa".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array, byte_array, ContractParameterType::Integer],
            ContractParameterType::Boolean,
        ),
        // recoverSecp256K1(messageHash, signature) -> ByteArray? (HF_Echidna).
        // Returns the compressed pubkey, or null on failure (signaled at runtime
        // via engine.set_native_return_null()).
        NativeMethod::new(
            "recoverSecp256K1".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array],
            byte_array,
        )
        .with_active_in(Hardfork::HfEchidna),
    ]
});

impl NativeContract for CryptoLib {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }

    fn name(&self) -> &str {
        "CryptoLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &CRYPTO_LIB_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // murmur32 takes (ByteArray data, Integer seed) and returns the 32-bit
        // hash little-endian (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        if method == "murmur32" {
            let data = args.first().ok_or_else(|| {
                CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
            })?;
            let seed_bytes = args.get(1).ok_or_else(|| {
                CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
            })?;
            // The VM integer seed is converted to `uint` exactly as C# `(uint)`
            // does: the low 32 bits (two's complement) of the BigInteger.
            let seed = (BigInt::from_signed_bytes_le(seed_bytes) & BigInt::from(u32::MAX))
                .to_u32()
                .unwrap_or(0);
            return Ok(murmur32(data, seed).to_le_bytes().to_vec());
        }

        if method == "verifyWithEd25519" {
            // C# VerifyWithEd25519(message, pubkey, signature): a wrong-length
            // signature (64) or pubkey (32) returns false, as does any
            // verification error (C# catches and returns false).
            let arg_err = || {
                CoreError::invalid_operation(
                    "CryptoLib::verifyWithEd25519 requires (message, pubkey, signature)",
                )
            };
            let message = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
            let pubkey = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
            let signature = args.get(2).map(Vec::as_slice).ok_or_else(arg_err)?;
            return Ok(vec![u8::from(verify_ed25519_method(message, pubkey, signature))]);
        }

        if method == "verifyWithECDsa" {
            // C# VerifyWithECDsa(message, pubkey, signature, curveHash): the
            // curveHash integer selects the (curve, hash) pair; Keccak-256 pairs
            // are only valid from HF_Cockatrice (the V0/V1 split).
            let arg_err = || {
                CoreError::invalid_operation(
                    "CryptoLib::verifyWithECDsa requires (message, pubkey, signature, curveHash)",
                )
            };
            let message = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
            let pubkey = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
            let signature = args.get(2).map(Vec::as_slice).ok_or_else(arg_err)?;
            let curve = args
                .get(3)
                .map(|b| BigInt::from_signed_bytes_le(b))
                .and_then(|b| b.to_u8())
                .ok_or_else(|| {
                    CoreError::invalid_operation("CryptoLib::verifyWithECDsa: curveHash out of range")
                })?;
            let allow_keccak = engine.is_hardfork_enabled(Hardfork::HfCockatrice);
            return Ok(vec![u8::from(verify_ecdsa_method(
                message, pubkey, signature, curve, allow_keccak,
            )?)]);
        }

        if method == "recoverSecp256K1" {
            // C# RecoverSecp256K1(messageHash, signature): the compressed pubkey,
            // or null on any recovery failure (the C# try/catch returns null).
            let arg_err = || {
                CoreError::invalid_operation(
                    "CryptoLib::recoverSecp256K1 requires (messageHash, signature)",
                )
            };
            let message_hash = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
            let signature = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
            return match recover_secp256k1_method(message_hash, signature) {
                Some(pubkey) => Ok(pubkey),
                None => {
                    // C# returns null; signal a null return so the dispatcher pushes
                    // StackItem::Null instead of an empty byte string.
                    engine.set_native_return_null();
                    Ok(Vec::new())
                }
            };
        }

        // Every CryptoLib hash method takes a single ByteArray and returns a
        // ByteArray; the engine marshals the argument to raw bytes and the
        // ByteArray return back to a VM ByteString.
        let data = args.first().ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} requires one argument"))
        })?;
        hash_method(method, data).ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib method '{method}' is not implemented"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn hash_methods_match_csharp_vectors() {
        // C# CryptoLib.{Sha256,RIPEMD160,Keccak256}(utf8("abc")).
        assert_eq!(
            hex(&hash_method("sha256", b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&hash_method("ripemd160", b"abc").unwrap()),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        assert_eq!(
            hex(&hash_method("keccak256", b"abc").unwrap()),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
        assert!(hash_method("not_a_method", b"abc").is_none());
    }

    #[test]
    fn murmur32_is_little_endian() {
        // MurmurHash3 x86 32 of empty input with seed 0 is 0 -> LE bytes 0,0,0,0
        // (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        assert_eq!(murmur32(b"", 0).to_le_bytes().to_vec(), vec![0u8, 0, 0, 0]);
        // Deterministic and non-trivial for a non-empty input.
        let h = murmur32(b"hello", 0);
        assert_eq!(murmur32(b"hello", 0), h);
        assert_eq!(h.to_le_bytes().len(), 4);
    }

    #[test]
    fn native_contract_surface_is_consistent() {
        let c = CryptoLib::new();
        assert_eq!(NativeContract::id(&c), -3);
        assert_eq!(NativeContract::name(&c), "CryptoLib");
        assert_eq!(NativeContract::hash(&c), *CRYPTO_LIB_HASH);

        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "sha256",
                "ripemd160",
                "keccak256",
                "murmur32",
                "verifyWithEd25519",
                "verifyWithECDsa",
                "recoverSecp256K1"
            ]
        );
        // keccak256 is hardfork-gated; the unconditional hashes are not.
        let keccak = c.methods().iter().find(|m| m.name == "keccak256").unwrap();
        assert_eq!(keccak.active_in, Some(Hardfork::HfCockatrice));
        assert!(c.methods().iter().all(|m| m.safe));
        // The hashes/murmur return ByteArray; verifyWithEd25519 is an Echidna
        // Boolean with three byte-array parameters.
        let ed = c.methods().iter().find(|m| m.name == "verifyWithEd25519").unwrap();
        assert_eq!(ed.return_type, ContractParameterType::Boolean);
        assert_eq!(ed.active_in, Some(Hardfork::HfEchidna));
        assert_eq!(ed.parameters.len(), 3);
        // verifyWithECDsa is always active (C# V0 from genesis); Boolean with
        // (message, pubkey, signature, curveHash) parameters.
        let ecdsa = c.methods().iter().find(|m| m.name == "verifyWithECDsa").unwrap();
        assert_eq!(ecdsa.return_type, ContractParameterType::Boolean);
        assert_eq!(ecdsa.active_in, None);
        assert_eq!(ecdsa.parameters.len(), 4);
        assert_eq!(ecdsa.parameters[3], ContractParameterType::Integer);
        // recoverSecp256K1 is HF_Echidna-gated, safe, (messageHash, signature) ->
        // ByteArray (nullable at runtime via set_native_return_null).
        let recover = c.methods().iter().find(|m| m.name == "recoverSecp256K1").unwrap();
        assert_eq!(recover.active_in, Some(Hardfork::HfEchidna));
        assert_eq!(recover.return_type, ContractParameterType::ByteArray);
        assert_eq!(recover.parameters, vec![ContractParameterType::ByteArray; 2]);
        assert!(recover.safe);
    }

    #[test]
    fn recover_secp256k1_returns_none_on_bad_input() {
        // The success path is round-trip-tested in neo-crypto
        // (recover_public_key_round_trips_and_rejects_bad_input); here we cover the
        // null path that maps to C# RecoverSecp256K1 returning null.
        let hash = [0x42u8; 32];
        assert!(recover_secp256k1_method(&hash, &[0u8; 10]).is_none()); // bad sig length
        assert!(recover_secp256k1_method(&[0u8; 31], &[0u8; 65]).is_none()); // bad hash length
        assert!(recover_secp256k1_method(&hash, &[0u8; 65]).is_none()); // invalid signature
    }

    #[test]
    fn verify_ecdsa_dispatch_gates_keccak_and_rejects_unknown_curve() {
        // The curve/hash dispatch + Cockatrice gate are tested here; the ECDSA
        // mechanics themselves are covered by neo-crypto's verify_signature_with_hash
        // tests (SHA-256 cross-check + Keccak-256 round-trips).
        let msg = b"message";
        let empty = b""; // malformed key/sig -> underlying verify yields false

        // Undefined curve byte -> error (C# KeyNotFound/ArgumentOutOfRange faults).
        assert!(verify_ecdsa_method(msg, empty, empty, 0x00, true).is_err());

        // SHA-256 curves (0x16/0x17) are valid at any height; malformed inputs
        // dispatch to a false result rather than faulting.
        assert!(!verify_ecdsa_method(msg, empty, empty, 0x16, false).unwrap());
        assert!(!verify_ecdsa_method(msg, empty, empty, 0x17, false).unwrap());

        // Keccak-256 curves (0x7A/0x7B) require Cockatrice: gated off -> fault.
        assert!(verify_ecdsa_method(msg, empty, empty, 0x7A, false).is_err());
        assert!(verify_ecdsa_method(msg, empty, empty, 0x7B, false).is_err());
        // Enabled -> dispatch (malformed inputs -> false).
        assert!(!verify_ecdsa_method(msg, empty, empty, 0x7A, true).unwrap());
        assert!(!verify_ecdsa_method(msg, empty, empty, 0x7B, true).unwrap());
    }

    fn hex_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn verify_ed25519_matches_rfc8032_test1() {
        // RFC 8032 Section 7.1, Test 1 (empty message).
        let pubkey =
            hex_bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let signature = hex_bytes(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
        let message: &[u8] = b"";
        assert!(verify_ed25519_method(message, &pubkey, &signature));

        // A tampered signature fails.
        let mut bad = signature.clone();
        bad[0] ^= 0x01;
        assert!(!verify_ed25519_method(message, &pubkey, &bad));

        // Wrong-length inputs return false without panicking (C# length guards).
        assert!(!verify_ed25519_method(message, &pubkey[..31], &signature));
        assert!(!verify_ed25519_method(message, &pubkey, &signature[..63]));
    }
}

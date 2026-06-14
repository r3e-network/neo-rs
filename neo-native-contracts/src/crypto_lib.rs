//! CryptoLib native contract (id -3).
//!
//! Implements the full C# `Neo.SmartContract.Native.CryptoLib` surface with
//! byte-for-byte parity, dispatched through the [`NativeContract`] trait so the
//! application engine can invoke it: the hash primitives (`sha256`,
//! `ripemd160`, `keccak256`, `murmur32`), signature verification
//! (`verifyWithECDsa`, `verifyWithEd25519`, `recoverSecp256K1`), and the
//! BLS12-381 operations (`bls12381Serialize` / `…Deserialize` / `…Equal` /
//! `…Add` / `…Mul` / `…Pairing`). The BLS points cross the native boundary as
//! `InteropInterface` objects (`neo_execution::Bls12381Interop`) wrapping the
//! canonical encoding, backed by `neo_crypto::Bls12381Point`. Every method
//! declared in [`CryptoLib::methods`] has a working implementation — no stubs.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::{Bls12381Point, Crypto, HashAlgorithm, Murmur3, NamedCurveHash, Secp256k1Crypto};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::CRYPTO_LIB_HASH;

/// The CryptoLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct CryptoLib;

impl CryptoLib {
    /// Stable native contract id (matches C# `CryptoLib`).
    pub const ID: i32 = -3;
    /// Stable native contract name (matches C# `CryptoLib.Name`).
    pub const NAME: &'static str = "CryptoLib";

    /// Construct a new `CryptoLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the CryptoLib script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the CryptoLib script hash.
    pub fn script_hash() -> UInt160 {
        *CRYPTO_LIB_HASH
    }

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
            && neo_crypto::ecc::EcdsaVerify::verify_ed25519(pubkey, message, signature)
                .unwrap_or(false)
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
            CoreError::invalid_operation(format!(
                "CryptoLib::verifyWithECDsa: unsupported curve {curve}"
            ))
        })?;
        if !allow_keccak && matches!(named.hash_algorithm(), HashAlgorithm::Keccak256) {
            return Err(CoreError::invalid_operation(
                "CryptoLib::verifyWithECDsa: Keccak256 curves require the Cockatrice hardfork",
            ));
        }
        Ok(neo_crypto::ecc::EcdsaVerify::verify_signature_with_hash(
            named.curve(),
            pubkey,
            message,
            signature,
            named.hash_algorithm(),
        )
        .unwrap_or(false))
    }

    /// Strict ECDSA verification (C# v3.10.0 `Crypto.VerifySignature`, used by
    /// `VerifyWithECDsaV2` from HF_Gorgon): an unsupported curve and a malformed
    /// signature length fault (rather than returning `false`). Keccak-256 curves are
    /// always available here (Gorgon is after Cockatrice). A valid-length signature
    /// that does not verify still returns `false`. (A right-length-but-invalid
    /// public key returns `false` here where C# would fault — a narrow divergence
    /// in this dormant Gorgon path, since the underlying verifier reports key parse
    /// failure as a non-match rather than an error.)
    fn verify_ecdsa_method_strict(
        message: &[u8],
        pubkey: &[u8],
        signature: &[u8],
        curve: u8,
    ) -> CoreResult<bool> {
        let named = NamedCurveHash::from_byte(curve).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "CryptoLib::verifyWithECDsa: unsupported curve {curve}"
            ))
        })?;
        if signature.len() != 64 {
            return Err(CoreError::invalid_operation(
                "CryptoLib::verifyWithECDsa: signature size should be 64 bytes",
            ));
        }
        Ok(neo_crypto::ecc::EcdsaVerify::verify_signature_with_hash(
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

    /// Deserializes the `idx`-th argument as a BLS12-381 point, faulting on a
    /// missing argument or a malformed/off-curve/wrong-subgroup encoding (C#
    /// `InteropInterface` binding + `FromCompressed`/`FromBytes` throw → VM fault).
    fn bls_point(method: &str, args: &[Vec<u8>], idx: usize) -> CoreResult<Bls12381Point> {
        let bytes = args.get(idx).ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} is missing argument {idx}"))
        })?;
        Bls12381Point::deserialize(bytes)
            .map_err(|e| CoreError::invalid_operation(format!("CryptoLib::{method}: {e}")))
    }

    /// Pure BLS12-381 `CryptoLib` dispatch (serialize / deserialize / equal / add /
    /// mul / pairing), split out so it can be unit-tested without an
    /// [`ApplicationEngine`]. Point arguments arrive as their canonical encoding
    /// (the dispatcher unwraps the `Bls12381Interop` operands to raw bytes); point
    /// results are returned as canonical bytes for the dispatcher to re-wrap as a
    /// `Bls12381Interop`. `bls12381Equal` returns a single boolean byte.
    ///
    /// Returns `Ok(None)` when `method` is not a BLS method (so the caller can fall
    /// through to the hash methods).
    fn bls12381_method(method: &str, args: &[Vec<u8>]) -> Option<CoreResult<Vec<u8>>> {
        let result = match method {
            // Serialize takes a point (InteropInterface) and returns its bytes; the
            // operand already arrives canonical, so round-tripping it normalizes.
            "bls12381Serialize" => Self::bls_point(method, args, 0).map(|p| p.serialize()),
            // Deserialize validates raw bytes into a point; the dispatcher re-wraps
            // the canonical encoding as an interop object.
            "bls12381Deserialize" => Self::bls_point(method, args, 0).map(|p| p.serialize()),
            "bls12381Equal" => Self::bls_point(method, args, 0).and_then(|a| {
                let b = Self::bls_point(method, args, 1)?;
                Ok(vec![u8::from(a.equals(&b))])
            }),
            "bls12381Add" => Self::bls_point(method, args, 0).and_then(|a| {
                let b = Self::bls_point(method, args, 1)?;
                a.add(&b).map(|sum| sum.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Add: {e}"))
                })
            }),
            "bls12381Mul" => Self::bls_point(method, args, 0).and_then(|p| {
                let mul = args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("CryptoLib::bls12381Mul is missing the multiplier")
                })?;
                // The `neg` flag (3rd arg) is a VM Boolean: any non-zero byte is true.
                let neg = args.get(2).is_some_and(|b| b.iter().any(|x| *x != 0));
                p.mul(mul, neg).map(|out| out.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Mul: {e}"))
                })
            }),
            "bls12381Pairing" => Self::bls_point(method, args, 0).and_then(|g1| {
                let g2 = Self::bls_point(method, args, 1)?;
                g1.pairing(&g2).map(|gt| gt.serialize()).map_err(|e| {
                    CoreError::invalid_operation(format!("CryptoLib::bls12381Pairing: {e}"))
                })
            }),
            _ => return None,
        };
        Some(result)
    }
}

// C# `CpuFee = 1 << 15` for sha256 / ripemd160 / keccak256.
const CPU_FEE_HASH: i64 = 1 << 15;

// C# CryptoLib BLS12-381 CpuFees (CryptoLib.BLS12_381.cs).
const CPU_FEE_BLS_SERIALIZE: i64 = 1 << 19;
const CPU_FEE_BLS_EQUAL: i64 = 1 << 5;
const CPU_FEE_BLS_ADD: i64 = 1 << 19;
const CPU_FEE_BLS_MUL: i64 = 1 << 21;
const CPU_FEE_BLS_PAIRING: i64 = 1 << 23;

static CRYPTO_LIB_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let byte_array = ContractParameterType::ByteArray;
    let interop = ContractParameterType::InteropInterface;
    vec![
        // Unconditional since genesis.
        NativeMethod::new(
            "sha256".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_parameter_names(["data"]),
        NativeMethod::new(
            "ripemd160".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array],
            byte_array,
        )
        .with_parameter_names(["data"]),
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
        .with_active_in(Hardfork::HfCockatrice)
        .with_parameter_names(["data"]),
        // murmur32(data: ByteArray, seed: Integer) -> ByteArray, C# CpuFee 1<<13.
        NativeMethod::new(
            "murmur32".to_string(),
            1 << 13,
            true,
            0,
            vec![byte_array, ContractParameterType::Integer],
            byte_array,
        )
        .with_parameter_names(["data", "seed"]),
        // verifyWithEd25519: dual manifest registration under one name (C#
        // v3.10.0 V0/V1). V0 = ActiveIn HF_Echidna, DeprecatedIn HF_Gorgon
        // (lenient: wrong-length sig/pubkey -> false). V1 = ActiveIn HF_Gorgon
        // (strict: wrong-length sig/pubkey faults). Exactly one is active at
        // any height; the strict gate is applied in invoke via HF_Gorgon.
        NativeMethod::new(
            "verifyWithEd25519".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array, byte_array],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_deprecated_in(Hardfork::HfGorgon)
        .with_parameter_names(["message", "pubkey", "signature"]),
        NativeMethod::new(
            "verifyWithEd25519".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![byte_array, byte_array, byte_array],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfGorgon)
        .with_parameter_names(["message", "pubkey", "signature"]),
        // verifyWithECDsa: dual manifest registration under one name (C# V0/V1).
        // V0 = `[ContractMethod(true, Hardfork.HF_Cockatrice, ...)]`:
        // genesis-active, DeprecatedIn Cockatrice, SHA-256 curves only, and its
        // fourth C# parameter is named `curve`. V1 = ActiveIn HF_Cockatrice,
        // adds the Keccak-256 curves, and renames the parameter `curveHash` —
        // so the manifests differ across the boundary even though the types
        // match. Exactly one is active at any height; the Keccak gate is
        // applied in invoke via the HF_Cockatrice check.
        NativeMethod::new(
            "verifyWithECDsa".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![
                byte_array,
                byte_array,
                byte_array,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_deprecated_in(Hardfork::HfCockatrice)
        .with_parameter_names(["message", "pubkey", "signature", "curve"]),
        NativeMethod::new(
            "verifyWithECDsa".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![
                byte_array,
                byte_array,
                byte_array,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfCockatrice)
        .with_deprecated_in(Hardfork::HfGorgon)
        .with_parameter_names(["message", "pubkey", "signature", "curveHash"]),
        // V2 = ActiveIn HF_Gorgon (C# VerifyWithECDsaV2): strict
        // Crypto.VerifySignature — a malformed signature length faults instead
        // of returning false.
        NativeMethod::new(
            "verifyWithECDsa".to_string(),
            CPU_FEE_HASH,
            true,
            0,
            vec![
                byte_array,
                byte_array,
                byte_array,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfGorgon)
        .with_parameter_names(["message", "pubkey", "signature", "curveHash"]),
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
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["messageHash", "signature"]),
        // BLS12-381 operations (genesis-active; CryptoLib.BLS12_381.cs). Points
        // are passed/returned as InteropInterface objects (Bls12381Interop).
        NativeMethod::new(
            "bls12381Serialize".to_string(),
            CPU_FEE_BLS_SERIALIZE,
            true,
            0,
            vec![interop],
            byte_array,
        )
        .with_parameter_names(["g"]),
        NativeMethod::new(
            "bls12381Deserialize".to_string(),
            CPU_FEE_BLS_SERIALIZE,
            true,
            0,
            vec![byte_array],
            interop,
        )
        .with_parameter_names(["data"]),
        NativeMethod::new(
            "bls12381Equal".to_string(),
            CPU_FEE_BLS_EQUAL,
            true,
            0,
            vec![interop, interop],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["x", "y"]),
        NativeMethod::new(
            "bls12381Add".to_string(),
            CPU_FEE_BLS_ADD,
            true,
            0,
            vec![interop, interop],
            interop,
        )
        .with_parameter_names(["x", "y"]),
        NativeMethod::new(
            "bls12381Mul".to_string(),
            CPU_FEE_BLS_MUL,
            true,
            0,
            vec![interop, byte_array, ContractParameterType::Boolean],
            interop,
        )
        .with_parameter_names(["x", "mul", "neg"]),
        NativeMethod::new(
            "bls12381Pairing".to_string(),
            CPU_FEE_BLS_PAIRING,
            true,
            0,
            vec![interop, interop],
            interop,
        )
        .with_parameter_names(["g1", "g2"]),
    ]
});

impl NativeContract for CryptoLib {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
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
            return Ok(Murmur3::murmur32(data, seed).to_le_bytes().to_vec());
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
            if engine.is_hardfork_enabled(Hardfork::HfGorgon) {
                // C# v3.10.0 VerifyWithEd25519V1 (strict): a wrong-length
                // signature (64) or public key (32) faults (FormatException)
                // instead of returning false.
                if signature.len() != 64 {
                    return Err(CoreError::invalid_operation(
                        "CryptoLib::verifyWithEd25519: signature size should be 64",
                    ));
                }
                if pubkey.len() != 32 {
                    return Err(CoreError::invalid_operation(
                        "CryptoLib::verifyWithEd25519: public key size should be 32",
                    ));
                }
                let ok = neo_crypto::ecc::EcdsaVerify::verify_ed25519(pubkey, message, signature)
                    .unwrap_or(false);
                return Ok(vec![u8::from(ok)]);
            }
            return Ok(vec![u8::from(Self::verify_ed25519_method(
                message, pubkey, signature,
            ))]);
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
                    CoreError::invalid_operation(
                        "CryptoLib::verifyWithECDsa: curveHash out of range",
                    )
                })?;
            if engine.is_hardfork_enabled(Hardfork::HfGorgon) {
                // C# v3.10.0 VerifyWithECDsaV2 (strict Crypto.VerifySignature):
                // a malformed signature length faults instead of returning false.
                return Ok(vec![u8::from(Self::verify_ecdsa_method_strict(
                    message, pubkey, signature, curve,
                )?)]);
            }
            let allow_keccak = engine.is_hardfork_enabled(Hardfork::HfCockatrice);
            return Ok(vec![u8::from(Self::verify_ecdsa_method(
                message,
                pubkey,
                signature,
                curve,
                allow_keccak,
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
            return match Self::recover_secp256k1_method(message_hash, signature) {
                Some(pubkey) => Ok(pubkey),
                None => {
                    // C# returns null; signal a null return so the dispatcher pushes
                    // StackItem::Null instead of an empty byte string.
                    engine.set_native_return_null();
                    Ok(Vec::new())
                }
            };
        }

        // BLS12-381 operations (bls12381Serialize/Deserialize/Equal/Add/Mul/
        // Pairing). Point operands/results cross the native boundary as their
        // canonical encoding, wrapped by the dispatcher as `Bls12381Interop`.
        if let Some(result) = Self::bls12381_method(method, args) {
            return result;
        }

        // Every CryptoLib hash method takes a single ByteArray and returns a
        // ByteArray; the engine marshals the argument to raw bytes and the
        // ByteArray return back to a VM ByteString.
        let data = args.first().ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} requires one argument"))
        })?;
        Self::hash_method(method, data).ok_or_else(|| {
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
            hex(&CryptoLib::hash_method("sha256", b"abc").unwrap()),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&CryptoLib::hash_method("ripemd160", b"abc").unwrap()),
            "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc"
        );
        assert_eq!(
            hex(&CryptoLib::hash_method("keccak256", b"abc").unwrap()),
            "4e03657aea45a94fc7d47ba826c8d667c0d1e6e33a64a036ec44f58fa12d6c45"
        );
        assert!(CryptoLib::hash_method("not_a_method", b"abc").is_none());
    }

    #[test]
    fn murmur32_is_little_endian() {
        // MurmurHash3 x86 32 of empty input with seed 0 is 0 -> LE bytes 0,0,0,0
        // (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        assert_eq!(
            Murmur3::murmur32(b"", 0).to_le_bytes().to_vec(),
            vec![0u8, 0, 0, 0]
        );
        // Deterministic and non-trivial for a non-empty input.
        let h = Murmur3::murmur32(b"hello", 0);
        assert_eq!(Murmur3::murmur32(b"hello", 0), h);
        assert_eq!(h.to_le_bytes().len(), 4);
    }

    #[test]
    fn native_contract_surface_is_consistent() {
        let c = CryptoLib::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "sha256",
                "ripemd160",
                "keccak256",
                "murmur32",
                "verifyWithEd25519", // V0 (ActiveIn Echidna, DeprecatedIn Gorgon)
                "verifyWithEd25519", // V1 (ActiveIn Gorgon, strict)
                "verifyWithECDsa",   // V0 (genesis, DeprecatedIn Cockatrice)
                "verifyWithECDsa",   // V1 (ActiveIn Cockatrice, DeprecatedIn Gorgon)
                "verifyWithECDsa",   // V2 (ActiveIn Gorgon, strict)
                "recoverSecp256K1",
                "bls12381Serialize",
                "bls12381Deserialize",
                "bls12381Equal",
                "bls12381Add",
                "bls12381Mul",
                "bls12381Pairing",
            ]
        );
        // keccak256 is hardfork-gated; the unconditional hashes are not.
        let keccak = c.methods().iter().find(|m| m.name == "keccak256").unwrap();
        assert_eq!(keccak.active_in, Some(Hardfork::HfCockatrice));
        assert!(c.methods().iter().all(|m| m.safe));
        // The hashes/murmur return ByteArray; verifyWithEd25519 is an Echidna
        // Boolean with three byte-array parameters.
        let ed = c
            .methods()
            .iter()
            .find(|m| m.name == "verifyWithEd25519")
            .unwrap();
        assert_eq!(ed.return_type, ContractParameterType::Boolean);
        assert_eq!(ed.active_in, Some(Hardfork::HfEchidna));
        assert_eq!(ed.parameters.len(), 3);
        // verifyWithECDsa is a triple registration (C# v3.10.0 V0/V1/V2): V0
        // runs from genesis until DeprecatedIn HF_Cockatrice with the fourth
        // parameter named `curve`; V1 is ActiveIn HF_Cockatrice (renames it
        // `curveHash`) until DeprecatedIn HF_Gorgon; V2 is ActiveIn HF_Gorgon
        // (strict). Types are identical across versions.
        let ecdsa: Vec<&NativeMethod> = c
            .methods()
            .iter()
            .filter(|m| m.name == "verifyWithECDsa")
            .collect();
        assert_eq!(ecdsa.len(), 3);
        let (v0, v1, v2) = (ecdsa[0], ecdsa[1], ecdsa[2]);
        assert_eq!(v0.active_in, None);
        assert_eq!(v0.deprecated_in, Some(Hardfork::HfCockatrice));
        assert_eq!(
            v0.parameter_names,
            ["message", "pubkey", "signature", "curve"]
        );
        assert_eq!(v1.active_in, Some(Hardfork::HfCockatrice));
        assert_eq!(v1.deprecated_in, Some(Hardfork::HfGorgon));
        assert_eq!(
            v1.parameter_names,
            ["message", "pubkey", "signature", "curveHash"]
        );
        assert_eq!(v2.active_in, Some(Hardfork::HfGorgon));
        assert_eq!(v2.deprecated_in, None);
        assert_eq!(
            v2.parameter_names,
            ["message", "pubkey", "signature", "curveHash"]
        );
        for m in &ecdsa {
            assert_eq!(m.return_type, ContractParameterType::Boolean);
            assert_eq!(m.parameters.len(), 4);
            assert_eq!(m.parameters[3], ContractParameterType::Integer);
        }
        // recoverSecp256K1 is HF_Echidna-gated, safe, (messageHash, signature) ->
        // ByteArray (nullable at runtime via set_native_return_null).
        let recover = c
            .methods()
            .iter()
            .find(|m| m.name == "recoverSecp256K1")
            .unwrap();
        assert_eq!(recover.active_in, Some(Hardfork::HfEchidna));
        assert_eq!(recover.return_type, ContractParameterType::ByteArray);
        assert_eq!(
            recover.parameters,
            vec![ContractParameterType::ByteArray; 2]
        );
        assert!(recover.safe);

        // BLS12-381 ABI (genesis-active, all safe; CryptoLib.BLS12_381.cs fees).
        let interop = ContractParameterType::InteropInterface;
        let bls = |name: &str| {
            c.methods()
                .iter()
                .find(|m| m.name == name)
                .cloned()
                .unwrap()
        };
        let ser = bls("bls12381Serialize");
        assert_eq!(ser.cpu_fee, 1 << 19);
        assert_eq!(ser.parameters, vec![interop]);
        assert_eq!(ser.return_type, ContractParameterType::ByteArray);
        let de = bls("bls12381Deserialize");
        assert_eq!(de.cpu_fee, 1 << 19);
        assert_eq!(de.parameters, vec![ContractParameterType::ByteArray]);
        assert_eq!(de.return_type, interop);
        let eq = bls("bls12381Equal");
        assert_eq!(eq.cpu_fee, 1 << 5);
        assert_eq!(eq.parameters, vec![interop, interop]);
        assert_eq!(eq.return_type, ContractParameterType::Boolean);
        let add = bls("bls12381Add");
        assert_eq!(add.cpu_fee, 1 << 19);
        assert_eq!(add.parameters, vec![interop, interop]);
        assert_eq!(add.return_type, interop);
        let mul = bls("bls12381Mul");
        assert_eq!(mul.cpu_fee, 1 << 21);
        assert_eq!(
            mul.parameters,
            vec![
                interop,
                ContractParameterType::ByteArray,
                ContractParameterType::Boolean
            ]
        );
        assert_eq!(mul.return_type, interop);
        let pairing = bls("bls12381Pairing");
        assert_eq!(pairing.cpu_fee, 1 << 23);
        assert_eq!(pairing.parameters, vec![interop, interop]);
        assert_eq!(pairing.return_type, interop);
        for name in [
            "bls12381Serialize",
            "bls12381Deserialize",
            "bls12381Equal",
            "bls12381Add",
            "bls12381Mul",
            "bls12381Pairing",
        ] {
            let m = bls(name);
            assert!(m.safe, "{name} is safe");
            assert_eq!(m.active_in, None, "{name} is genesis-active");
        }
    }

    // BLS12-381 dispatch vectors (a subset of UT_CryptoLib; s_gtHex == e(g1,g2)).
    // The full byte-exact arithmetic is verified in neo_crypto::bls12381_point —
    // these confirm the native dispatch maps each method to the right operation,
    // parses the (point, scalar, neg) arguments correctly, and returns canonical
    // bytes / boolean bytes the way the engine marshaling expects.
    const BLS_G1: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb";
    const BLS_G2: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";
    const BLS_GT: &str = "0f41e58663bf08cf068672cbd01a7ec73baca4d72ca93544deff686bfd6df543d48eaa24afe47e1efde449383b67663104c581234d086a9902249b64728ffd21a189e87935a954051c7cdba7b3872629a4fafc05066245cb9108f0242d0fe3ef03350f55a7aefcd3c31b4fcb6ce5771cc6a0e9786ab5973320c806ad360829107ba810c5a09ffdd9be2291a0c25a99a211b8b424cd48bf38fcef68083b0b0ec5c81a93b330ee1a677d0d15ff7b984e8978ef48881e32fac91b93b47333e2ba5706fba23eb7c5af0d9f80940ca771b6ffd5857baaf222eb95a7d2809d61bfe02e1bfd1b68ff02f0b8102ae1c2d5d5ab1a19f26337d205fb469cd6bd15c3d5a04dc88784fbb3d0b2dbdea54d43b2b73f2cbb12d58386a8703e0f948226e47ee89d018107154f25a764bd3c79937a45b84546da634b8f6be14a8061e55cceba478b23f7dacaa35c8ca78beae9624045b4b601b2f522473d171391125ba84dc4007cfbf2f8da752f7c74185203fcca589ac719c34dffbbaad8431dad1c1fb597aaa5193502b86edb8857c273fa075a50512937e0794e1e65a7617c90d8bd66065b1fffe51d7a579973b1315021ec3c19934f1368bb445c7c2d209703f239689ce34c0378a68e72a6b3b216da0e22a5031b54ddff57309396b38c881c4c849ec23e87089a1c5b46e5110b86750ec6a532348868a84045483c92b7af5af689452eafabf1a8943e50439f1d59882a98eaa0170f1250ebd871fc0a92a7b2d83168d0d727272d441befa15c503dd8e90ce98db3e7b6d194f60839c508a84305aaca1789b6";

    #[test]
    fn bls12381_dispatch_matches_crypto_layer() {
        let g1 = hex::decode(BLS_G1).unwrap();
        let g2 = hex::decode(BLS_G2).unwrap();
        let gt = hex::decode(BLS_GT).unwrap();
        let call = |m: &str, args: &[Vec<u8>]| {
            CryptoLib::bls12381_method(m, args)
                .expect("is a BLS method")
                .expect("method succeeds")
        };
        let scalar = |n: u8| {
            let mut s = [0u8; 32];
            s[0] = n;
            s.to_vec()
        };

        // Deserialize normalizes to canonical bytes; Serialize returns them.
        assert_eq!(call("bls12381Deserialize", std::slice::from_ref(&g1)), g1);
        assert_eq!(call("bls12381Serialize", std::slice::from_ref(&g1)), g1);

        // Pairing e(g1,g2) == s_gtHex — the headline C# vector through dispatch.
        assert_eq!(call("bls12381Pairing", &[g1.clone(), g2.clone()]), gt);

        // Add(gt,gt) == Mul(gt, 2): cross-checks the Add and Mul wiring.
        assert_eq!(
            call("bls12381Add", &[gt.clone(), gt.clone()]),
            call("bls12381Mul", &[gt.clone(), scalar(2), vec![0]])
        );

        // gt*3 + gt*(-3) == gt*0 (identity): verifies Mul's `neg` flag + Add.
        let pos = call("bls12381Mul", &[gt.clone(), scalar(3), vec![0]]);
        let neg = call("bls12381Mul", &[gt.clone(), scalar(3), vec![1]]);
        let identity = call("bls12381Mul", &[gt.clone(), scalar(0), vec![0]]);
        assert_eq!(
            call(
                "bls12381Equal",
                &[call("bls12381Add", &[pos, neg]), identity]
            ),
            vec![1u8]
        );

        // Equal: same point true, cross-group false.
        assert_eq!(call("bls12381Equal", &[g1.clone(), g1.clone()]), vec![1u8]);
        assert_eq!(call("bls12381Equal", &[g1.clone(), g2.clone()]), vec![0u8]);

        // Faults (Err → VM fault): malformed point, swapped pairing operands,
        // wrong scalar length.
        assert!(
            CryptoLib::bls12381_method("bls12381Deserialize", &[vec![0u8; 47]])
                .unwrap()
                .is_err()
        );
        assert!(
            CryptoLib::bls12381_method("bls12381Pairing", &[g2.clone(), g1.clone()])
                .unwrap()
                .is_err()
        );
        assert!(
            CryptoLib::bls12381_method("bls12381Mul", &[gt.clone(), vec![0u8; 31], vec![0]])
                .unwrap()
                .is_err()
        );

        // A non-BLS method is not handled here (falls through to hash dispatch).
        assert!(CryptoLib::bls12381_method("sha256", &[]).is_none());
    }

    #[test]
    fn recover_secp256k1_returns_none_on_bad_input() {
        // The success path is round-trip-tested in neo-crypto
        // (recover_public_key_round_trips_and_rejects_bad_input); here we cover the
        // null path that maps to C# RecoverSecp256K1 returning null.
        let hash = [0x42u8; 32];
        assert!(CryptoLib::recover_secp256k1_method(&hash, &[0u8; 10]).is_none()); // bad sig length
        assert!(CryptoLib::recover_secp256k1_method(&[0u8; 31], &[0u8; 65]).is_none()); // bad hash length
        assert!(CryptoLib::recover_secp256k1_method(&hash, &[0u8; 65]).is_none()); // invalid signature
    }

    #[test]
    fn verify_ecdsa_dispatch_gates_keccak_and_rejects_unknown_curve() {
        // The curve/hash dispatch + Cockatrice gate are tested here; the ECDSA
        // mechanics themselves are covered by neo-crypto's verify_signature_with_hash
        // tests (SHA-256 cross-check + Keccak-256 round-trips).
        let msg = b"message";
        let empty = b""; // malformed key/sig -> underlying verify yields false

        // Undefined curve byte -> error (C# KeyNotFound/ArgumentOutOfRange faults).
        assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x00, true).is_err());

        // SHA-256 curves (0x16/0x17) are valid at any height; malformed inputs
        // dispatch to a false result rather than faulting.
        assert!(!CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x16, false).unwrap());
        assert!(!CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x17, false).unwrap());

        // Keccak-256 curves (0x7A/0x7B) require Cockatrice: gated off -> fault.
        assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x7A, false).is_err());
        assert!(CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x7B, false).is_err());
        // Enabled -> dispatch (malformed inputs -> false).
        assert!(!CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x7A, true).unwrap());
        assert!(!CryptoLib::verify_ecdsa_method(msg, empty, empty, 0x7B, true).unwrap());
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
        let pubkey = hex_bytes("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let signature = hex_bytes(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
        let message: &[u8] = b"";
        assert!(CryptoLib::verify_ed25519_method(
            message, &pubkey, &signature
        ));

        // A tampered signature fails.
        let mut bad = signature.clone();
        bad[0] ^= 0x01;
        assert!(!CryptoLib::verify_ed25519_method(message, &pubkey, &bad));

        // Wrong-length inputs return false without panicking (C# length guards).
        assert!(!CryptoLib::verify_ed25519_method(
            message,
            &pubkey[..31],
            &signature
        ));
        assert!(!CryptoLib::verify_ed25519_method(
            message,
            &pubkey,
            &signature[..63]
        ));
    }
}

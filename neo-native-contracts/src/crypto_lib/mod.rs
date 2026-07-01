//! # neo-native-contracts::crypto_lib
//!
//! Native CryptoLib interop surface and verification helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_config::Hardfork;
use neo_crypto::{Bls12381Point, Crypto, HashAlgorithm, Murmur3, NamedCurveHash, Secp256k1Crypto};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::CRYPTO_LIB_HASH;

mod metadata;

native_contract_handle!(
    /// The CryptoLib native contract.
    pub struct CryptoLib {
        id: -3,
        contract_name: "CryptoLib",
        hash: CRYPTO_LIB_HASH,
    }
);

impl CryptoLib {
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

    /// Pure secp256k1 public-key recovery with C# `RecoverSecp256K1` semantics:
    /// returns the 33-byte compressed public key, or `None` when recovery fails (C#
    /// wraps `Crypto.ECRecover` in try/catch and returns `null`). Split out so the
    /// success/null decision can be unit tested without an [`ApplicationEngine`].
    fn recover_secp256k1_method(message_hash: &[u8], signature: &[u8]) -> Option<Vec<u8>> {
        Secp256k1Crypto::recover_public_key(message_hash, signature).ok()
    }

    /// C# native binding converts `uint seed` with `(uint)p.GetInteger()`, which
    /// faults on negative or wider-than-uint BigInteger values.
    fn murmur32_seed(seed_bytes: &[u8]) -> CoreResult<u32> {
        BigInt::from_signed_bytes_le(seed_bytes)
            .to_u32()
            .ok_or_else(|| CoreError::invalid_operation("CryptoLib::murmur32: seed out of range"))
    }

    fn murmur32_method(data: &[u8], seed_bytes: &[u8]) -> CoreResult<Vec<u8>> {
        Ok(Murmur3::murmur32(data, Self::murmur32_seed(seed_bytes)?)
            .to_le_bytes()
            .to_vec())
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

impl NativeContract for CryptoLib {
    native_contract_identity!(CryptoLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::CRYPTO_LIB_METHODS
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
            return Self::murmur32_method(data, seed_bytes);
        }

        if method == "verifyWithEd25519" {
            // C# VerifyWithEd25519(message, pubkey, signature): a wrong-length
            // signature or pubkey returns false, as does any
            // verification error (C# catches and returns false).
            let arg_err = || {
                CoreError::invalid_operation(
                    "CryptoLib::verifyWithEd25519 requires (message, pubkey, signature)",
                )
            };
            let message = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
            let pubkey = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
            let signature = args.get(2).map(Vec::as_slice).ok_or_else(arg_err)?;
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
#[path = "../tests/crypto_lib/mod.rs"]
mod tests;

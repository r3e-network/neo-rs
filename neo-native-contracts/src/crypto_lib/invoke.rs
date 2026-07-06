//! CryptoLib native-method dispatch.
//!
//! Keeps engine-aware routing and hardfork gates separate from the pure hash,
//! signature, recovery, murmur, and BLS helper implementations.

use super::CryptoLib;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl CryptoLib {
    pub(super) fn invoke_native(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        crate::support::invoke::dispatch_by_name(
            self,
            &super::metadata::CRYPTO_LIB_METHOD_BINDINGS,
            engine,
            method,
            args,
        )
        .unwrap_or_else(|| {
            Err(CoreError::invalid_operation(format!(
                "CryptoLib method '{method}' is not implemented"
            )))
        })
    }

    fn single_byte_array_arg<'a>(method: &str, args: &'a [Vec<u8>]) -> CoreResult<&'a [u8]> {
        args.first().map(Vec::as_slice).ok_or_else(|| {
            CoreError::invalid_operation(format!("CryptoLib::{method} requires one argument"))
        })
    }

    pub(super) fn invoke_sha256(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Ok(Self::sha256_method(Self::single_byte_array_arg(
            "sha256", args,
        )?))
    }

    pub(super) fn invoke_ripemd160(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Ok(Self::ripemd160_method(Self::single_byte_array_arg(
            "ripemd160",
            args,
        )?))
    }

    pub(super) fn invoke_keccak256(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Ok(Self::keccak256_method(Self::single_byte_array_arg(
            "keccak256",
            args,
        )?))
    }

    pub(super) fn invoke_murmur32(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // murmur32 takes (ByteArray data, Integer seed) and returns the 32-bit
        // hash little-endian (C# `BinaryPrimitives.WriteUInt32LittleEndian`).
        let data = args.first().ok_or_else(|| {
            CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
        })?;
        let seed_bytes = args.get(1).ok_or_else(|| {
            CoreError::invalid_operation("CryptoLib::murmur32 requires two arguments")
        })?;
        Self::murmur32_method(data, seed_bytes)
    }

    pub(super) fn invoke_verify_with_ed25519(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# VerifyWithEd25519(message, pubkey, signature): V0
        // (Echidna..Gorgon) returns false for wrong lengths; V1
        // (Gorgon+) faults on wrong lengths.
        let arg_err = || {
            CoreError::invalid_operation(
                "CryptoLib::verifyWithEd25519 requires (message, pubkey, signature)",
            )
        };
        let message = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
        let pubkey = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
        let signature = args.get(2).map(Vec::as_slice).ok_or_else(arg_err)?;
        let verified = if engine.is_hardfork_enabled(Hardfork::HfGorgon) {
            Self::verify_ed25519_gorgon_method(message, pubkey, signature)?
        } else {
            Self::verify_ed25519_method(message, pubkey, signature)
        };
        Ok(vec![u8::from(verified)])
    }

    pub(super) fn invoke_verify_with_ecdsa(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# VerifyWithECDsa(message, pubkey, signature, curveHash): the
        // curveHash integer selects the (curve, hash) pair; Keccak-256 pairs
        // are only valid from HF_Cockatrice. HF_Gorgon switches from
        // VerifySignatureV0 to VerifySignature and faults on bad format.
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
        let gorgon = engine.is_hardfork_enabled(Hardfork::HfGorgon);
        Ok(vec![u8::from(Self::verify_ecdsa_method(
            message,
            pubkey,
            signature,
            curve,
            allow_keccak,
            gorgon,
        )?)])
    }

    pub(super) fn invoke_recover_secp256k1(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# RecoverSecp256K1(messageHash, signature): the compressed pubkey,
        // or null on any recovery failure (the C# try/catch returns null).
        let arg_err = || {
            CoreError::invalid_operation(
                "CryptoLib::recoverSecp256K1 requires (messageHash, signature)",
            )
        };
        let message_hash = args.first().map(Vec::as_slice).ok_or_else(arg_err)?;
        let signature = args.get(1).map(Vec::as_slice).ok_or_else(arg_err)?;
        match Self::recover_secp256k1_method(message_hash, signature) {
            Some(pubkey) => Ok(pubkey),
            None => {
                // C# returns null; signal a null return so the dispatcher pushes
                // StackItem::Null instead of an empty byte string.
                engine.set_native_return_null();
                Ok(Vec::new())
            }
        }
    }

    pub(super) fn invoke_bls12381_serialize(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_serialize_method(args)
    }

    pub(super) fn invoke_bls12381_deserialize(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_deserialize_method(args)
    }

    pub(super) fn invoke_bls12381_equal(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_equal_method(args)
    }

    pub(super) fn invoke_bls12381_add(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_add_method(args)
    }

    pub(super) fn invoke_bls12381_mul(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_mul_method(args)
    }

    pub(super) fn invoke_bls12381_pairing(
        &self,
        _engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Self::bls12381_pairing_method(args)
    }
}

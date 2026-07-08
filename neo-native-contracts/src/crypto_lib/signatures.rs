//! CryptoLib signature verification and public-key recovery helpers.
//!
//! The helpers mirror C# hardfork-specific fault/false/null behavior and stay
//! independent from the native invocation wrapper.

use super::CryptoLib;
use neo_crypto::{ECPoint, HashAlgorithm, NamedCurveHash, Secp256k1Crypto};
use neo_error::{CoreError, CoreResult};

impl CryptoLib {
    /// Pure Ed25519 verification with C# `VerifyWithEd25519` semantics: a
    /// wrong-length signature (64) or public key (32), or any verification
    /// error, yields `false`. Split out so it can be unit tested without an
    /// [`neo_execution::ApplicationEngine`].
    pub(super) fn verify_ed25519_method(message: &[u8], pubkey: &[u8], signature: &[u8]) -> bool {
        signature.len() == 64
            && pubkey.len() == 32
            && neo_crypto::ecc::EcdsaVerify::verify_ed25519(pubkey, message, signature)
                .unwrap_or(false)
    }

    /// C# HF_Gorgon `VerifyWithEd25519V1`: wrong-length signature/public key
    /// throws a format exception instead of returning false.
    pub(super) fn verify_ed25519_gorgon_method(
        message: &[u8],
        pubkey: &[u8],
        signature: &[u8],
    ) -> CoreResult<bool> {
        if signature.len() != 64 {
            return Err(CoreError::invalid_operation(
                "CryptoLib::verifyWithEd25519: signature size should be 64 bytes",
            ));
        }
        if pubkey.len() != 32 {
            return Err(CoreError::invalid_operation(
                "CryptoLib::verifyWithEd25519: public key size should be 32 bytes",
            ));
        }
        neo_crypto::ecc::EcdsaVerify::verify_ed25519(pubkey, message, signature)
            .map_err(|e| CoreError::invalid_operation(format!("CryptoLib::verifyWithEd25519: {e}")))
    }

    /// Pure ECDSA verification with C# `VerifyWithECDsa` semantics, split out
    /// so the curve/hash dispatch can be unit tested without an
    /// [`neo_execution::ApplicationEngine`].
    ///
    /// `allow_keccak` reflects the `HF_Cockatrice` hardfork (the V0/V1 split):
    /// before Cockatrice only the SHA-256 named curves are valid, so a
    /// Keccak-256 curve faults (C# `VerifyWithECDsaV0` throws
    /// `ArgumentOutOfRangeException`); an undefined `curve` byte also faults
    /// (C# `s_curves[...]` `KeyNotFoundException`). `gorgon` reflects the
    /// `HF_Gorgon` V2 split: pre-Gorgon malformed public keys or signatures
    /// return `Ok(false)`, while Gorgon and later propagate format errors so
    /// the VM faults.
    pub(super) fn verify_ecdsa_method(
        message: &[u8],
        pubkey: &[u8],
        signature: &[u8],
        curve: u8,
        allow_keccak: bool,
        gorgon: bool,
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
        if gorgon {
            // V2 (HF_Gorgon..), dormant on v3.10.1 mainnet/testnet. C#
            // `VerifyWithECDsaV2` calls `Crypto.VerifySignature` with no catch, so
            // a bad signature length or an invalid public key both FAULT.
            if signature.len() != 64 {
                return Err(CoreError::invalid_operation(
                    "CryptoLib::verifyWithECDsa: signature size should be 64 bytes",
                ));
            }
            ECPoint::decode(pubkey, named.curve()).map_err(|e| {
                CoreError::invalid_operation(format!("CryptoLib::verifyWithECDsa: {e}"))
            })?;
            return neo_crypto::ecc::EcdsaVerify::verify_signature_with_hash(
                named.curve(),
                pubkey,
                message,
                signature,
                named.hash_algorithm(),
            )
            .map_err(|e| CoreError::invalid_operation(format!("CryptoLib::verifyWithECDsa: {e}")));
        }

        // V0/V1 (genesis..HF_Gorgon), the ACTIVE v3.10.1 path. C#
        // `VerifyWithECDsaV0/V1` wrap `Crypto.VerifySignatureV0(msg, sig, pubkey,
        // curve, hash)` in `catch(ArgumentException)`. That overload decodes the
        // key via `ECPoint.DecodePoint` FIRST (as the argument to the inner
        // `VerifySignatureV0`), and only then does the inner method check the
        // signature length. `DecodePoint` throws `FormatException`/
        // `ArithmeticException`/`IndexOutOfRangeException` for a malformed/off-curve/
        // empty key - none are `ArgumentException`, so a bad key is NOT caught and
        // the VM FAULTS, even when the signature length is also wrong. A good key
        // with a wrong-length or non-verifying signature returns false. Reproduce
        // that exact order: decode (fault) before the length check.
        ECPoint::decode(pubkey, named.curve()).map_err(|e| {
            CoreError::invalid_operation(format!("CryptoLib::verifyWithECDsa: {e}"))
        })?;
        if signature.len() != 64 {
            return Ok(false);
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

    /// Pure secp256k1 public-key recovery with C# `RecoverSecp256K1`
    /// semantics: returns the 33-byte compressed public key, or `None` when
    /// recovery fails (C# wraps `Crypto.ECRecover` in try/catch and returns
    /// `null`). Split out so the success/null decision can be unit tested
    /// without an [`neo_execution::ApplicationEngine`].
    pub(super) fn recover_secp256k1_method(
        message_hash: &[u8],
        signature: &[u8],
    ) -> Option<Vec<u8>> {
        // C# `Crypto.ECRecover` requires exactly 65 bytes (`r||s||v`) and throws
        // `ArgumentException` on any other length (`if (signature.Length != 65)
        // throw`). `RecoverSecp256K1` wraps it in try/catch and returns null on
        // exception. A 64-byte EIP-2098 compact form is NOT accepted by the C#
        // consensus method - accepting it would fork any contract that branches on
        // the result.
        if signature.len() != 65 {
            return None;
        }
        Secp256k1Crypto::recover_public_key(message_hash, signature).ok()
    }
}

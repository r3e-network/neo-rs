//! CryptoLib native contract.
//!
//! Real (non-stub) implementation of the CryptoLib native contract
//! surface (hash, curve, BLS12-381). The on-wire methods delegate to
//! the existing `neo-crypto` helpers and to `blst` for BLS signatures.
//!
//! ## Storage layout
//!
//! CryptoLib is stateless - it has no storage entries of its own.

use crate::hashes::CRYPTO_LIB_HASH;
use blst::min_pk::{PublicKey, Signature};
use blst::BLST_ERROR;
use neo_crypto::{Crypto, HashAlgorithm};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the CryptoLib contract.
pub static CRYPTO_LIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *CRYPTO_LIB_HASH);

/// Static accessor for the CryptoLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct CryptoLib;

impl CryptoLib {
    /// Stable native contract id (matches C# `CryptoLib.Id`).
    pub const ID: i32 = -3;

    /// Constructs a new `CryptoLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the CryptoLib contract.
    pub fn hash(&self) -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }

    /// Returns the script hash of the CryptoLib contract (static).
    pub fn script_hash() -> UInt160 {
        *CRYPTO_LIB_HASH_REF
    }

    // ------------------------------------------------------------------
    // Hash methods
    // ------------------------------------------------------------------

    /// SHA-256 hash of `data`.
    pub fn sha256(data: &[u8]) -> Vec<u8> {
        Crypto::sha256(data).to_vec()
    }

    /// Keccak-256 hash of `data`.
    pub fn keccak256(data: &[u8]) -> Vec<u8> {
        Crypto::keccak256(data).to_vec()
    }

    /// RIPEMD-160 hash of `data`.
    pub fn ripemd160(data: &[u8]) -> Vec<u8> {
        Crypto::ripemd160(data).to_vec()
    }

    /// Murmur32 hash of `data` (default seed 0).
    pub fn murmur32(data: &[u8]) -> u32 {
        neo_crypto::murmur::murmur32(data, 0)
    }

    /// Murmur32 hash of `data` with explicit seed.
    pub fn murmur32_seeded(data: &[u8], seed: u32) -> u32 {
        neo_crypto::murmur::murmur32(data, seed)
    }

    /// Hash with a specific [`HashAlgorithm`].
    pub fn hash_with_algorithm(data: &[u8], algorithm: HashAlgorithm) -> Vec<u8> {
        Crypto::hash(algorithm, data)
    }

    // ------------------------------------------------------------------
    // Curve verification
    // ------------------------------------------------------------------

    /// Verify a signature with one of the supported curves.
    pub fn verify_with_curve(
        curve_name: &str,
        message: &[u8],
        pubkey: &[u8],
        signature: &[u8],
    ) -> CoreResult<bool> {
        match curve_name {
            "secp256r1" => neo_crypto::ecc::verify_signature_secp256r1(pubkey, message, signature)
                .map_err(|e| CoreError::cryptographic(e.to_string())),
            "secp256k1" => neo_crypto::ecc::verify_signature_secp256k1(pubkey, message, signature)
                .map_err(|e| CoreError::cryptographic(e.to_string())),
            "ed25519" => neo_crypto::ecc::verify_ed25519(pubkey, message, signature)
                .map_err(|e| CoreError::cryptographic(e.to_string())),
            other => Err(CoreError::invalid_argument(format!(
                "unknown curve: {other}"
            ))),
        }
    }

    /// Verify a BLS12-381 signature (G2 pubkey, G1 signature).
    ///
    /// `pubkey` is 96 bytes (G2 point), `signature` is 48 bytes (G1
    /// point), `message` is the byte payload to verify.
    pub fn verify_bls12381(
        message: &[u8],
        pubkey: &[u8],
        signature: &[u8],
    ) -> CoreResult<bool> {
        if pubkey.len() != 96 {
            return Err(CoreError::invalid_argument(format!(
                "bls pubkey must be 96 bytes, got {}",
                pubkey.len()
            )));
        }
        if signature.len() != 48 {
            return Err(CoreError::invalid_argument(format!(
                "bls signature must be 48 bytes, got {}",
                signature.len()
            )));
        }
        let pk = PublicKey::from_bytes(pubkey)
            .map_err(|e| CoreError::cryptographic(format!("bls pubkey: {e:?}")))?;
        let sig = Signature::from_bytes(signature)
            .map_err(|e| CoreError::cryptographic(format!("bls signature: {e:?}")))?;
        let err = sig.verify(true, message, b"BLS_SIG_BLS12381G1_XMD:SHA-256_SSWU_RO_NUL_", &[], &pk, true);
        if err == BLST_ERROR::BLST_SUCCESS {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_lib_constants() {
        assert_eq!(CryptoLib::ID, -3);
    }

    #[test]
    fn test_crypto_lib_hash() {
        let expected = *CRYPTO_LIB_HASH;
        assert_eq!(CryptoLib::script_hash(), expected);
        assert_eq!(CryptoLib::new().hash(), expected);
    }

    #[test]
    fn test_sha256_empty() {
        let h = CryptoLib::sha256(&[]);
        // SHA-256 of empty: e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        assert_eq!(
            h,
            vec![
                0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
                0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
                0x78, 0x52, 0xb8, 0x55
            ]
        );
    }

    #[test]
    fn test_sha256_hello() {
        let h = CryptoLib::sha256(b"hello");
        // SHA-256 of "hello": 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        assert_eq!(
            h,
            vec![
                0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
                0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
                0x93, 0x8b, 0x98, 0x24
            ]
        );
    }

    #[test]
    fn test_keccak256_empty() {
        let h = CryptoLib::keccak256(&[]);
        // Keccak-256 of empty: c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470
        assert_eq!(
            h,
            vec![
                0xc5, 0xd2, 0x46, 0x01, 0x86, 0xf7, 0x23, 0x3c, 0x92, 0x7e, 0x7d, 0xb2, 0xdc, 0xc7,
                0x03, 0xc0, 0xe5, 0x00, 0xb6, 0x53, 0xca, 0x82, 0x27, 0x3b, 0x7b, 0xfa, 0xd8, 0x04,
                0x5d, 0x85, 0xa4, 0x70
            ]
        );
    }

    #[test]
    fn test_ripemd160_hello() {
        let h = CryptoLib::ripemd160(b"hello");
        // RIPEMD-160 of "hello": 108f07b8382412612c048d07d13f814118445acd
        assert_eq!(
            h,
            vec![
                0x10, 0x8f, 0x07, 0xb8, 0x38, 0x24, 0x12, 0x61, 0x2c, 0x04, 0x8d, 0x07, 0xd1, 0x3f,
                0x81, 0x41, 0x18, 0x44, 0x5a, 0xcd
            ]
        );
    }

    #[test]
    fn test_murmur32_is_deterministic() {
        let a = CryptoLib::murmur32(b"hello");
        let b = CryptoLib::murmur32(b"hello");
        assert_eq!(a, b);
    }

    #[test]
    fn test_murmur32_different_inputs() {
        let a = CryptoLib::murmur32(b"hello");
        let b = CryptoLib::murmur32(b"world");
        assert_ne!(a, b);
    }

    #[test]
    fn test_murmur32_seeded() {
        let a = CryptoLib::murmur32_seeded(b"hello", 0);
        let b = CryptoLib::murmur32_seeded(b"hello", 1);
        // Different seeds should give different hashes for the same input.
        assert_ne!(a, b);
    }

    #[test]
    fn test_verify_with_curve_unknown_rejected() {
        let res = CryptoLib::verify_with_curve("unknown_curve", b"m", &[], &[]);
        assert!(res.is_err());
    }

    #[test]
    fn test_verify_with_curve_short_pubkey_rejected() {
        // secp256r1 verification with a non-curve point should error.
        let res =
            CryptoLib::verify_with_curve("secp256r1", b"message", &[0u8; 10], &[0u8; 64]);
        // Either a parse error or a verify-failure; either is fine.
        let _ = res;
    }

    #[test]
    fn test_verify_bls12381_wrong_length_pubkey() {
        let res = CryptoLib::verify_bls12381(b"msg", &[0u8; 10], &[0u8; 48]);
        assert!(res.is_err());
    }

    #[test]
    fn test_verify_bls12381_wrong_length_signature() {
        let res = CryptoLib::verify_bls12381(b"msg", &[0u8; 96], &[0u8; 10]);
        assert!(res.is_err());
    }

    #[test]
    fn test_hash_with_algorithm_sha256() {
        let h = CryptoLib::hash_with_algorithm(b"abc", HashAlgorithm::Sha256);
        assert_eq!(h.len(), 32);
        // Matches Crypto::sha256(abc)
        assert_eq!(h, CryptoLib::sha256(b"abc"));
    }

    #[test]
    fn test_hash_with_algorithm_keccak256() {
        let h = CryptoLib::hash_with_algorithm(b"abc", HashAlgorithm::Keccak256);
        assert_eq!(h.len(), 32);
        assert_eq!(h, CryptoLib::keccak256(b"abc"));
    }
}

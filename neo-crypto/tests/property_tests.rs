//! Property-based tests for neo-crypto
//!
//! These tests use proptest to verify:
//! - Hash consistency (same input always produces same hash)
//! - Crypto operations (sign then verify returns true)
//! - Various hash algorithms

use neo_crypto::{
    Crypto,
    crypto_utils::{Base58, Ed25519Crypto, Hex, Secp256r1Crypto},
};
use proptest::prelude::*;

proptest! {
    // =========================================================================
    // Hash Consistency Tests
    // =========================================================================

    /// Test that SHA256 produces consistent hashes
    #[test]
    fn test_sha256_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::sha256(&data);
        let hash2 = Crypto::sha256(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that SHA512 produces consistent hashes
    #[test]
    fn test_sha512_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::sha512(&data);
        let hash2 = Crypto::sha512(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that RIPEMD160 produces consistent hashes
    #[test]
    fn test_ripemd160_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::ripemd160(&data);
        let hash2 = Crypto::ripemd160(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that Hash160 produces consistent hashes
    #[test]
    fn test_hash160_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::hash160(&data);
        let hash2 = Crypto::hash160(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that Hash256 produces consistent hashes
    #[test]
    fn test_hash256_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::hash256(&data);
        let hash2 = Crypto::hash256(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that Keccak256 produces consistent hashes
    #[test]
    fn test_keccak256_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::keccak256(&data);
        let hash2 = Crypto::keccak256(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that Blake2b produces consistent hashes
    #[test]
    fn test_blake2b_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::blake2b(&data);
        let hash2 = Crypto::blake2b(&data);
        prop_assert_eq!(hash1, hash2);
    }

    /// Test that Blake2s produces consistent hashes
    #[test]
    fn test_blake2s_consistency(data in any::<Vec<u8>>()) {
        let hash1 = Crypto::blake2s(&data);
        let hash2 = Crypto::blake2s(&data);
        prop_assert_eq!(hash1, hash2);
    }

    // =========================================================================
    // Encoding Roundtrip Tests
    // =========================================================================

    /// Test that Base58 encoding roundtrips correctly
    #[test]
    fn test_base58_roundtrip(data in any::<Vec<u8>>()) {
        let encoded = Base58::encode(&data);
        let decoded = Base58::decode(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }

    /// Test that Hex encoding roundtrips correctly
    #[test]
    fn test_hex_roundtrip(data in any::<Vec<u8>>()) {
        let encoded = Hex::encode(&data);
        let decoded = Hex::decode(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }

    /// Test that Base58Check encoding roundtrips correctly
    #[test]
    fn test_base58check_roundtrip(data in any::<Vec<u8>>()) {
        let encoded = Base58::encode_check(&data);
        let decoded = Base58::decode_check(&encoded).unwrap();
        prop_assert_eq!(data, decoded);
    }

    // =========================================================================
    // Secp256r1 (P-256) Crypto Tests
    // =========================================================================

    /// Test that secp256r1 sign then verify returns true
    #[test]
    fn test_secp256r1_sign_verify(message in any::<Vec<u8>>()) {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();

        let signature = Secp256r1Crypto::sign(&message, &private_key).unwrap();
        let verified = Secp256r1Crypto::verify(&message, &signature, &public_key).unwrap();

        prop_assert!(verified);
    }

    /// Test that secp256r1 verification fails for wrong message
    #[test]
    fn test_secp256r1_wrong_message_fails(
        msg1 in any::<Vec<u8>>(),
        msg2 in any::<Vec<u8>>()
    ) {
        prop_assume!(!msg1.is_empty() || !msg2.is_empty());
        prop_assume!(msg1 != msg2);

        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();

        let signature = Secp256r1Crypto::sign(&msg1, &private_key).unwrap();
        let verified = Secp256r1Crypto::verify(&msg2, &signature, &public_key).unwrap();

        prop_assert!(!verified);
    }

    // =========================================================================
    // Ed25519 Crypto Tests
    // =========================================================================

    /// Test that Ed25519 sign then verify returns true
    #[test]
    fn test_ed25519_sign_verify(message in any::<Vec<u8>>()) {
        let private_key = Ed25519Crypto::generate_private_key();
        let public_key = Ed25519Crypto::derive_public_key(&private_key).unwrap();

        let signature = Ed25519Crypto::sign(&message, &private_key).unwrap();
        let verified = Ed25519Crypto::verify(&message, &signature, &public_key).unwrap();

        prop_assert!(verified);
    }

    /// Test that Ed25519 verification fails for wrong message
    #[test]
    fn test_ed25519_wrong_message_fails(
        msg1 in any::<Vec<u8>>(),
        msg2 in any::<Vec<u8>>()
    ) {
        prop_assume!(!msg1.is_empty() || !msg2.is_empty());
        prop_assume!(msg1 != msg2);

        let private_key = Ed25519Crypto::generate_private_key();
        let public_key = Ed25519Crypto::derive_public_key(&private_key).unwrap();

        let signature = Ed25519Crypto::sign(&msg1, &private_key).unwrap();
        let verified = Ed25519Crypto::verify(&msg2, &signature, &public_key).unwrap();

        prop_assert!(!verified);
    }
}

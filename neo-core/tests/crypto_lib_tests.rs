//! CryptoLib native contract unit tests matching C# UT_CryptoLib
//!
//! Tests for Neo.SmartContract.Native.CryptoLib functionality.

use neo_core::cryptography::{Bls12381Crypto, NeoHash};
use neo_core::smart_contract::native::crypto_lib::CryptoLib;
use neo_core::smart_contract::native::NativeContract;

/// Tests that CryptoLib has correct contract ID (-3)
#[test]
fn test_crypto_lib_id() {
    let crypto = CryptoLib::new();
    assert_eq!(crypto.id(), -3, "CryptoLib ID should be -3");
}

/// Tests that CryptoLib has correct name
#[test]
fn test_crypto_lib_name() {
    let crypto = CryptoLib::new();
    assert_eq!(crypto.name(), "CryptoLib", "CryptoLib name should match");
}

/// Tests SHA256 hash function
#[test]
fn test_sha256() {
    let data = b"Hello, World!";
    let hash = NeoHash::sha256(data);

    // Known SHA256 hash of "Hello, World!"
    let expected =
        hex::decode("dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "SHA256 hash should match"
    );
}

/// Tests SHA256 with empty input
#[test]
fn test_sha256_empty() {
    let data = b"";
    let hash = NeoHash::sha256(data);

    // SHA256 of empty string
    let expected =
        hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "SHA256 of empty should match"
    );
}

/// Tests RIPEMD160 hash function
#[test]
fn test_ripemd160() {
    let data = b"Hello, World!";
    let hash = NeoHash::ripemd160(data);

    // Known RIPEMD160 hash of "Hello, World!" (verified with Python hashlib and OpenSSL)
    let expected = hex::decode("527a6a4b9a6da75607546842e0e00105350b1aaf").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "RIPEMD160 hash should match"
    );
}

/// Tests RIPEMD160 with empty input
#[test]
fn test_ripemd160_empty() {
    let data = b"";
    let hash = NeoHash::ripemd160(data);

    // RIPEMD160 of empty string
    let expected = hex::decode("9c1185a5c5e9fc54612808977ee8f548b2258d31").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "RIPEMD160 of empty should match"
    );
}

/// Tests Hash160 (SHA256 + RIPEMD160)
#[test]
fn test_hash160() {
    let data = b"test";
    let hash = NeoHash::hash160(data);

    // Hash160 is RIPEMD160(SHA256(data))
    let sha256_result = NeoHash::sha256(data);
    let expected = NeoHash::ripemd160(&sha256_result);

    assert_eq!(
        hash, expected,
        "Hash160 should equal RIPEMD160(SHA256(data))"
    );
}

/// Tests Hash256 (double SHA256)
#[test]
fn test_hash256() {
    let data = b"test";
    let hash = NeoHash::hash256(data);

    // Hash256 is SHA256(SHA256(data))
    let first_hash = NeoHash::sha256(data);
    let expected = NeoHash::sha256(&first_hash);

    assert_eq!(hash, expected, "Hash256 should equal SHA256(SHA256(data))");
}

/// Tests CryptoLib methods are registered
#[test]
fn test_crypto_lib_methods() {
    let crypto = CryptoLib::new();
    let methods = crypto.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(method_names.contains(&"sha256"), "Should have sha256");
    assert!(method_names.contains(&"ripemd160"), "Should have ripemd160");
    assert!(
        method_names.contains(&"verifyWithECDsa"),
        "Should have verifyWithECDsa"
    );
}

/// Tests BLS12-381 key derivation
#[test]
fn test_bls12381_derive_public_key() {
    // Test private key (32 bytes)
    let private_key: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    let result = Bls12381Crypto::derive_public_key(&private_key);
    assert!(result.is_ok(), "Should derive public key successfully");

    let public_key = result.unwrap();
    assert_eq!(
        public_key.len(),
        96,
        "BLS public key should be 96 bytes (compressed G2)"
    );
}

/// Tests BLS12-381 sign and verify
#[test]
fn test_bls12381_sign_verify() {
    let private_key: [u8; 32] = [
        0x2b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    let message = b"test message";

    // Derive public key
    let public_key =
        Bls12381Crypto::derive_public_key(&private_key).expect("Should derive public key");

    // Sign message
    let signature = Bls12381Crypto::sign(message, &private_key).expect("Should sign message");

    assert_eq!(
        signature.len(),
        48,
        "BLS signature should be 48 bytes (compressed G1)"
    );

    // Verify signature
    let is_valid = Bls12381Crypto::verify(message, &signature, &public_key)
        .expect("Verification should not error");

    assert!(is_valid, "Signature should be valid");
}

/// Tests BLS12-381 verify with wrong message fails
#[test]
fn test_bls12381_verify_wrong_message() {
    let private_key: [u8; 32] = [
        0x2b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    let message = b"test message";
    let wrong_message = b"wrong message";

    let public_key =
        Bls12381Crypto::derive_public_key(&private_key).expect("Should derive public key");

    let signature = Bls12381Crypto::sign(message, &private_key).expect("Should sign message");

    // Verify with wrong message should fail
    let is_valid = Bls12381Crypto::verify(wrong_message, &signature, &public_key)
        .expect("Verification should not error");

    assert!(!is_valid, "Signature should be invalid for wrong message");
}

/// Tests BLS12-381 signature aggregation
#[test]
fn test_bls12381_aggregate_signatures() {
    let private_key1: [u8; 32] = [0x01; 32];
    let private_key2: [u8; 32] = [0x02; 32];

    let message = b"shared message";

    let sig1 = Bls12381Crypto::sign(message, &private_key1).expect("Should sign with key 1");
    let sig2 = Bls12381Crypto::sign(message, &private_key2).expect("Should sign with key 2");

    let aggregated =
        Bls12381Crypto::aggregate_signatures(&[sig1, sig2]).expect("Should aggregate signatures");

    assert_eq!(
        aggregated.len(),
        48,
        "Aggregated signature should be 48 bytes"
    );
}

/// Tests consistent hashing produces same results
#[test]
fn test_hash_consistency() {
    let data = b"consistent test data";

    // Hash the same data multiple times
    let hash1 = NeoHash::sha256(data);
    let hash2 = NeoHash::sha256(data);
    let hash3 = NeoHash::sha256(data);

    assert_eq!(hash1, hash2, "SHA256 should be deterministic");
    assert_eq!(hash2, hash3, "SHA256 should be deterministic");
}

/// Tests hash of different data produces different results
#[test]
fn test_hash_uniqueness() {
    let data1 = b"data one";
    let data2 = b"data two";

    let hash1 = NeoHash::sha256(data1);
    let hash2 = NeoHash::sha256(data2);

    assert_ne!(
        hash1, hash2,
        "Different data should produce different hashes"
    );
}

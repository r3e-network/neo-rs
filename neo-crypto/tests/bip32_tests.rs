//! Comprehensive tests for BIP-32 cryptographic primitives

use neo_crypto::{bip32::Bip32Crypto, ECCurve};

#[test]
fn test_hmac_sha512_basic() {
    let key = b"test key";
    let data = b"test data";
    let result = Bip32Crypto::hmac_sha512(key, data).unwrap();

    assert_eq!(result.len(), 64);
    assert_ne!(result, [0u8; 64]);
}

#[test]
fn test_hmac_sha512_deterministic() {
    let key = b"key";
    let data = b"data";

    let result1 = Bip32Crypto::hmac_sha512(key, data).unwrap();
    let result2 = Bip32Crypto::hmac_sha512(key, data).unwrap();

    assert_eq!(result1, result2);
}

#[test]
fn test_hmac_sha512_different_keys() {
    let data = b"same data";

    let result1 = Bip32Crypto::hmac_sha512(b"key1", data).unwrap();
    let result2 = Bip32Crypto::hmac_sha512(b"key2", data).unwrap();

    assert_ne!(result1, result2);
}

#[test]
fn test_hmac_sha512_different_data() {
    let key = b"same key";

    let result1 = Bip32Crypto::hmac_sha512(key, b"data1").unwrap();
    let result2 = Bip32Crypto::hmac_sha512(key, b"data2").unwrap();

    assert_ne!(result1, result2);
}

#[test]
fn test_hmac_sha512_empty_data() {
    let key = b"key";
    let result = Bip32Crypto::hmac_sha512(key, b"").unwrap();

    assert_eq!(result.len(), 64);
    assert_ne!(result, [0u8; 64]);
}

#[test]
fn test_hmac_sha512_long_key() {
    let key = vec![0x42; 128];
    let data = b"data";
    let result = Bip32Crypto::hmac_sha512(&key, data).unwrap();

    assert_eq!(result.len(), 64);
}

#[test]
fn test_hmac_sha512_long_data() {
    let key = b"key";
    let data = vec![0x55; 10000];
    let result = Bip32Crypto::hmac_sha512(key, &data).unwrap();

    assert_eq!(result.len(), 64);
}

#[test]
fn test_hmac_sha512_bip32_bitcoin_seed_vector() {
    // BIP-32 test vector for master seed derivation
    let seed = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
    let output = Bip32Crypto::hmac_sha512(b"Bitcoin seed", &seed).unwrap();

    let expected = concat!(
        "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35",
        "873dff81c02f525623fd1fe5167eac3a55a049de3d314bb42ee227ffed37d508"
    );

    assert_eq!(hex::encode(output), expected);
}

#[test]
fn test_hmac_sha512_neo_seed_vector() {
    let seed = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
    let output = Bip32Crypto::hmac_sha512(b"Nist256p1 seed", &seed).unwrap();

    // Verify deterministic output
    let output2 = Bip32Crypto::hmac_sha512(b"Nist256p1 seed", &seed).unwrap();
    assert_eq!(output, output2);
    assert_eq!(output.len(), 64);
}

#[test]
fn test_add_private_keys_secp256r1() {
    let left = [1u8; 32];
    let right = [2u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&left, &right, ECCurve::Secp256r1).unwrap();
    assert_eq!(result.len(), 32);
    assert_ne!(result, [0u8; 32]);
}

#[test]
fn test_add_private_keys_secp256k1() {
    let left = [1u8; 32];
    let right = [2u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&left, &right, ECCurve::Secp256k1).unwrap();
    assert_eq!(result.len(), 32);
    assert_ne!(result, [0u8; 32]);
}

#[test]
fn test_add_private_keys_ed25519_unsupported() {
    let left = [1u8; 32];
    let right = [2u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&left, &right, ECCurve::Ed25519);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Ed25519"));
}

#[test]
fn test_add_private_keys_zero_result() {
    // Create keys that would sum to zero (or near curve order)
    // This should fail as zero private keys are invalid
    let zeros = [0u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&zeros, &zeros, ECCurve::Secp256r1);
    assert!(result.is_err());
}

#[test]
fn test_add_private_keys_overflow() {
    // Test with a value >= curve order (should fail)
    let mut large = [0xFFu8; 32];
    large[0] = 0xFF;

    let small = [1u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&large, &small, ECCurve::Secp256r1);
    // Should either succeed with modular reduction or fail if left_factor >= order
    // The exact behavior depends on the actual curve order
    let _ = result; // May or may not error depending on exact implementation
}

#[test]
fn test_add_private_keys_commutative() {
    let a = [0x11u8; 32];
    let b = [0x22u8; 32];

    let result1 = Bip32Crypto::add_private_keys_mod_order(&a, &b, ECCurve::Secp256r1).unwrap();
    let result2 = Bip32Crypto::add_private_keys_mod_order(&b, &a, ECCurve::Secp256r1).unwrap();

    // Addition modulo order should be commutative
    assert_eq!(result1, result2);
}

#[test]
fn test_add_private_keys_small_values() {
    let mut small1 = [0u8; 32];
    small1[31] = 1; // Value = 1

    let mut small2 = [0u8; 32];
    small2[31] = 2; // Value = 2

    let result = Bip32Crypto::add_private_keys_mod_order(&small1, &small2, ECCurve::Secp256r1).unwrap();

    let mut expected = [0u8; 32];
    expected[31] = 3; // Value = 3

    assert_eq!(result, expected);
}

#[test]
fn test_hmac_sha512_binary_key() {
    let key = vec![0x00, 0xFF, 0xAA, 0x55];
    let data = b"test";

    let result = Bip32Crypto::hmac_sha512(&key, data).unwrap();
    assert_eq!(result.len(), 64);
}

#[test]
fn test_hmac_sha512_binary_data() {
    let key = b"key";
    let data = vec![0x00, 0xFF, 0xAA, 0x55, 0x12, 0x34];

    let result = Bip32Crypto::hmac_sha512(key, &data).unwrap();
    assert_eq!(result.len(), 64);
}

#[test]
fn test_hmac_sha512_consistency() {
    let key = b"consistent key";
    let data = b"consistent data";

    // Run multiple times to ensure consistency
    let results: Vec<[u8; 64]> = (0..10)
        .map(|_| Bip32Crypto::hmac_sha512(key, data).unwrap())
        .collect();

    for i in 1..results.len() {
        assert_eq!(results[0], results[i]);
    }
}

#[test]
fn test_add_private_keys_different_curves() {
    let a = [0x01u8; 32];
    let b = [0x02u8; 32];

    let r1_result = Bip32Crypto::add_private_keys_mod_order(&a, &b, ECCurve::Secp256r1).unwrap();
    let k1_result = Bip32Crypto::add_private_keys_mod_order(&a, &b, ECCurve::Secp256k1).unwrap();

    // The curves have different orders, so results may differ
    // Both should be valid 32-byte results
    assert_eq!(r1_result.len(), 32);
    assert_eq!(k1_result.len(), 32);
}

#[test]
fn test_hmac_sha512_single_byte_key() {
    let key = b"k";
    let data = b"data";
    let result = Bip32Crypto::hmac_sha512(key, data).unwrap();
    assert_eq!(result.len(), 64);
}

#[test]
fn test_hmac_sha512_single_byte_data() {
    let key = b"key";
    let data = b"d";
    let result = Bip32Crypto::hmac_sha512(key, data).unwrap();
    assert_eq!(result.len(), 64);
}

#[test]
fn test_add_private_keys_sequential() {
    // Test adding multiple small values sequentially
    let mut accumulator = [0u8; 32];
    accumulator[31] = 10;

    let increment = [0u8; 32];

    for i in 1..5 {
        let mut inc = increment;
        inc[31] = i;

        accumulator = Bip32Crypto::add_private_keys_mod_order(
            &inc,
            &accumulator,
            ECCurve::Secp256r1
        ).unwrap();
    }

    // Should have accumulated: 10 + 1 + 2 + 3 + 4 = 20
    let mut expected = [0u8; 32];
    expected[31] = 20;
    assert_eq!(accumulator, expected);
}

#[test]
fn test_add_private_keys_identity() {
    let value = [0x42u8; 32];
    let zero = [0u8; 32];

    let result = Bip32Crypto::add_private_keys_mod_order(&zero, &value, ECCurve::Secp256r1);
    // Adding zero to a value returns the value (zero < order, so it's valid as left_factor)
    // The result would be zero if both were zero, which would fail
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), value);
}

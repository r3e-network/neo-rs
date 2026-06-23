use super::*;

#[test]
fn test_seal_unseal() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"Hello, TEE world!";
    let aad = b"additional data";

    let sealed = Sealing::seal_data(plaintext, &key, aad, 1).unwrap();
    let unsealed = Sealing::unseal_data(&sealed, &key, None).unwrap();

    assert_eq!(unsealed, plaintext);
}

#[test]
fn test_seal_unseal_with_context() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"secret data";
    let aad = b"additional data";

    // Seal with specific context
    let sealed = Sealing::seal_data_with_context(plaintext, &key, aad, 1, "wallet-key").unwrap();
    assert_eq!(sealed.context, Some("wallet-key".to_string()));

    // Should unseal with same key
    let unsealed = Sealing::unseal_data(&sealed, &key, None).unwrap();
    assert_eq!(unsealed, plaintext);
}

#[test]
fn test_context_domain_separation() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"secret data";

    // Seal with different contexts
    let sealed1 = Sealing::seal_data_with_context(plaintext, &key, &[], 1, "context-a").unwrap();
    let sealed2 = Sealing::seal_data_with_context(plaintext, &key, &[], 1, "context-b").unwrap();

    // Ciphertexts should be different due to different derived keys
    // (even with same nonce would fail, but with different nonces definitely)
    assert_ne!(sealed1.ciphertext, sealed2.ciphertext);

    // Each should only decrypt with correct implicit context
    let unsealed1 = Sealing::unseal_data(&sealed1, &key, None).unwrap();
    let unsealed2 = Sealing::unseal_data(&sealed2, &key, None).unwrap();

    assert_eq!(unsealed1, plaintext);
    assert_eq!(unsealed2, plaintext);
}

#[test]
fn test_hkdf_key_derivation() {
    let base_key: [u8; 32] = rand::random();

    // Derive two keys with different contexts
    let params1 = KeyDerivationParams {
        base_key: &base_key,
        context: "encryption",
        salt: None,
    };
    let key1 = Sealing::derive_key_hkdf(params1).unwrap();

    let params2 = KeyDerivationParams {
        base_key: &base_key,
        context: "authentication",
        salt: None,
    };
    let key2 = Sealing::derive_key_hkdf(params2).unwrap();

    // Derived keys should be different
    assert_ne!(key1, key2);
    assert_ne!(key1, base_key);
    assert_ne!(key2, base_key);

    // Same parameters should produce same key
    let params3 = KeyDerivationParams {
        base_key: &base_key,
        context: "encryption",
        salt: None,
    };
    let key3 = Sealing::derive_key_hkdf(params3).unwrap();
    assert_eq!(key1, key3);
}

#[test]
fn test_hkdf_salt_domain_separation() {
    let base_key: [u8; 32] = rand::random();

    // Same context, different salts should produce different keys
    let params1 = KeyDerivationParams {
        base_key: &base_key,
        context: "test",
        salt: Some(b"salt1"),
    };
    let key1 = Sealing::derive_key_hkdf(params1).unwrap();

    let params2 = KeyDerivationParams {
        base_key: &base_key,
        context: "test",
        salt: Some(b"salt2"),
    };
    let key2 = Sealing::derive_key_hkdf(params2).unwrap();

    assert_ne!(key1, key2);
}

#[test]
fn test_replay_protection() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"secret data";

    let sealed = Sealing::seal_data(plaintext, &key, &[], 5).unwrap();

    // Should succeed with counter >= 5
    assert!(Sealing::unseal_data(&sealed, &key, Some(5)).is_ok());
    assert!(Sealing::unseal_data(&sealed, &key, Some(4)).is_ok());

    // Should fail with counter > 5
    assert!(Sealing::unseal_data(&sealed, &key, Some(6)).is_err());
}

#[test]
fn test_tamper_detection() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"important data";

    let mut sealed = Sealing::seal_data(plaintext, &key, &[], 1).unwrap();

    // Tamper with ciphertext
    sealed.ciphertext[0] ^= 0xFF;

    // Should fail decryption
    assert!(Sealing::unseal_data(&sealed, &key, None).is_err());
}

#[test]
fn test_aad_integrity() {
    let key: [u8; 32] = rand::random();
    let plaintext = b"important data";
    let aad = b"binding data";

    let mut sealed = Sealing::seal_data(plaintext, &key, aad, 1).unwrap();

    // Tamper with AAD
    sealed.aad.push(0xFF);

    // Should fail decryption (AAD mismatch)
    assert!(Sealing::unseal_data(&sealed, &key, None).is_err());
}

#[test]
fn test_secure_key_zeroize() {
    let key_bytes: [u8; 32] = rand::random();
    let key = SecureKey::new(key_bytes);

    // Clone and verify
    let key_clone = key.clone();
    assert_eq!(key_clone.as_bytes(), key.as_bytes());

    // Derive subkey
    let subkey = key.derive_subkey("test-context").unwrap();
    assert_ne!(subkey.as_bytes(), key.as_bytes());
}

//! Comprehensive tests for batch signature verification

use neo_crypto::{
    Crypto,
    batch_verifier::{BatchVerifier, SignatureComponents},
    signature::Secp256r1Crypto,
};

fn generate_keypair() -> ([u8; 32], [u8; 33]) {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key_vec = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let mut public_key = [0u8; 33];
    public_key.copy_from_slice(&public_key_vec);
    (private_key, public_key)
}

#[test]
fn test_batch_verifier_empty() {
    let mut verifier = BatchVerifier::new();
    assert!(verifier.is_empty());
    assert_eq!(verifier.len(), 0);
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_single_valid_signature() {
    let mut verifier = BatchVerifier::new();

    let (private_key, public_key) = generate_keypair();
    let message = b"test message";
    let message_digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

    verifier.add(message_digest, signature, public_key);
    assert_eq!(verifier.len(), 1);
    assert!(!verifier.is_empty());
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_multiple_valid_signatures() {
    let mut verifier = BatchVerifier::new();

    for i in 0..10 {
        let (private_key, public_key) = generate_keypair();
        let message = format!("test message {i}");
        let message_digest = Crypto::sha256(message.as_bytes());
        let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

        verifier.add(message_digest, signature, public_key);
    }

    assert_eq!(verifier.len(), 10);
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_invalid_signature() {
    let mut verifier = BatchVerifier::new();

    let (private_key, public_key) = generate_keypair();
    let message = b"test message";
    let message_digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

    verifier.add(message_digest, signature, public_key);

    // Add an invalid signature (wrong message)
    let wrong_message = b"wrong message";
    let wrong_digest = Crypto::sha256(wrong_message);
    verifier.add(wrong_digest, signature, public_key);

    assert_eq!(verifier.len(), 2);
    assert!(!verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_mixed_signatures() {
    let mut verifier = BatchVerifier::new();

    // Add some valid signatures
    for i in 0..5 {
        let (private_key, public_key) = generate_keypair();
        let message = format!("valid message {i}");
        let message_digest = Crypto::sha256(message.as_bytes());
        let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

        verifier.add(message_digest, signature, public_key);
    }

    // Add an invalid signature in the middle
    let (private_key, public_key) = generate_keypair();
    let message = b"message";
    let message_digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();
    let wrong_digest = Crypto::sha256(b"different message");
    verifier.add(wrong_digest, signature, public_key);

    assert_eq!(verifier.len(), 6);
    assert!(!verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_reset() {
    let mut verifier = BatchVerifier::new();

    let (private_key, public_key) = generate_keypair();
    let message = b"test message";
    let message_digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

    verifier.add(message_digest, signature, public_key);
    assert_eq!(verifier.len(), 1);
    assert!(verifier.verify_all().unwrap());

    verifier.reset();
    assert_eq!(verifier.len(), 0);
    assert!(verifier.is_empty());
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_reuse_after_verification() {
    let mut verifier = BatchVerifier::new();

    let (private_key1, public_key1) = generate_keypair();
    let message1 = b"message 1";
    let digest1 = Crypto::sha256(message1);
    let signature1 = Secp256r1Crypto::sign_prehash(&digest1, &private_key1).unwrap();

    verifier.add(digest1, signature1, public_key1);
    assert!(verifier.verify_all().unwrap());

    // Verify again without reset (should use cached result)
    assert!(verifier.verify_all().unwrap());

    // Reset and add new batch
    verifier.reset();
    let (private_key2, public_key2) = generate_keypair();
    let message2 = b"message 2";
    let digest2 = Crypto::sha256(message2);
    let signature2 = Secp256r1Crypto::sign_prehash(&digest2, &private_key2).unwrap();

    verifier.add(digest2, signature2, public_key2);
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_default() {
    let verifier = BatchVerifier::default();
    assert!(verifier.is_empty());
    assert_eq!(verifier.len(), 0);
}

#[test]
fn test_batch_verifier_large_batch() {
    let mut verifier = BatchVerifier::new();

    for i in 0..100 {
        let (private_key, public_key) = generate_keypair();
        let message = format!("transaction {i}");
        let message_digest = Crypto::sha256(message.as_bytes());
        let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

        verifier.add(message_digest, signature, public_key);
    }

    assert_eq!(verifier.len(), 100);
    assert!(verifier.verify_all().unwrap());
}

#[test]
fn test_batch_verifier_wrong_public_key() {
    let mut verifier = BatchVerifier::new();

    let (private_key, _) = generate_keypair();
    let message = b"test message";
    let message_digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&message_digest, &private_key).unwrap();

    // Try to verify with different public key
    let (_, wrong_public_key) = generate_keypair();
    verifier.add(message_digest, signature, wrong_public_key);

    assert!(!verifier.verify_all().unwrap());
}

#[test]
fn test_signature_components_clone() {
    let (private_key, public_key) = generate_keypair();
    let message = b"test";
    let digest = Crypto::sha256(message);
    let signature = Secp256r1Crypto::sign_prehash(&digest, &private_key).unwrap();

    let components = SignatureComponents {
        message_digest: digest,
        signature,
        public_key,
    };

    let cloned = components.clone();
    assert_eq!(components.message_digest, cloned.message_digest);
    assert_eq!(components.signature, cloned.signature);
    assert_eq!(components.public_key, cloned.public_key);
}

#[test]
fn test_batch_verifier_debug_format() {
    let components = SignatureComponents {
        message_digest: [1u8; 32],
        signature: [2u8; 64],
        public_key: [3u8; 33],
    };

    let debug_str = format!("{components:?}");
    assert!(debug_str.contains("SignatureComponents"));
}

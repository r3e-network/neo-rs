//! Focused property-based checks for the Rust Neo cryptography primitives.
//!
//! The original scaffold attempted to mirror every C# compatibility surface, but it
//! depended on helper APIs that do not exist in the current crate layout (for example
//! `neo_cryptography::bls` and bespoke timing hooks).  This replacement keeps the
//! intent—exercise the real hashing and ECDSA implementations over a wide range of
//! inputs—while staying within the supported public API.

use neo_cryptography::{hash::Hash160, hash::Hash256, ECDsa};
use proptest::prelude::*;

proptest! {
    /// Hash256 must be stable: hashing the same payload twice should always
    /// produce the same 32‑byte digest.
    #[test]
    fn hash256_is_deterministic(input in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let first = Hash256::hash(&input);
        let second = Hash256::hash(&input);
        let len = second.len();
        prop_assert_eq!(first, second);
        prop_assert_eq!(len, 32);
    }

    /// Hash160 uses SHA256 + RIPEMD160, so the result must always be 20 bytes and
    /// deterministic.
    #[test]
    fn hash160_is_deterministic(input in proptest::collection::vec(any::<u8>(), 0..1024)) {
        let first = Hash160::hash(&input);
        let second = Hash160::hash(&input);
        let len = second.len();
        prop_assert_eq!(first, second);
        prop_assert_eq!(len, 20);
    }

    /// Signing a random message with a valid private key must verify with the
    /// derived public key.  We restrict the key generation to values that the
    /// secp256r1 constructor accepts.
    #[test]
    fn ecdsa_sign_and_verify(
        message in proptest::collection::vec(any::<u8>(), 1..256),
        private_key_bytes in proptest::array::uniform32(any::<u8>())
    ) {
        // Skip keys that the curve rejects.
        prop_assume!(ECDsa::validate_private_key(&private_key_bytes));

        let public_key = ECDsa::derive_public_key(&private_key_bytes)
            .expect("public key derivation to succeed for a valid private key");

        let signature = ECDsa::sign(&message, &private_key_bytes)
            .expect("signing to succeed");

        let verifies = ECDsa::verify(&message, &signature, &public_key)
            .expect("verification to run");

        prop_assert!(verifies);

        // Flipping a bit in the message should invalidate most signatures.
        if !message.is_empty() {
            let mut tampered = message.clone();
            tampered[0] ^= 0x01;
            let verifies_tampered = ECDsa::verify(&tampered, &signature, &public_key)
                .expect("verification to run");
            prop_assert!(!verifies_tampered);
        }
    }
}

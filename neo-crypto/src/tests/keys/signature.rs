use super::{NEOFS_ECDSA_SHA512_PREFIX, Secp256k1Crypto, Secp256r1Crypto};
use crate::{Crypto, ECCurve, HashAlgorithm};

#[test]
fn canonicalize_signature_raw_der_and_errors() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"canonicalize test";
    let raw = Secp256r1Crypto::sign(message, &private_key).unwrap();

    // Raw r||s path: canonicalizes to a low-s 64-byte sig that still verifies.
    let canon = Secp256r1Crypto::canonicalize_signature(&raw, false).unwrap();
    assert_eq!(canon.len(), 64);
    assert_eq!(canon, Secp256r1Crypto::normalize_low_s(&raw).unwrap());
    assert!(Secp256r1Crypto::verify(message, &canon, &public_key).unwrap());

    // DER path (e.g. GCP libkmsp11) yields the same canonical r||s.
    let der = p256::ecdsa::Signature::from_slice(&raw).unwrap().to_der();
    let from_der = Secp256r1Crypto::canonicalize_signature(der.as_bytes(), true).unwrap();
    assert_eq!(from_der, canon);

    // A wrong-length raw signature is rejected, not silently accepted.
    assert!(Secp256r1Crypto::canonicalize_signature(&[0u8; 10], false).is_err());
}

#[test]
fn test_secp256k1_operations() {
    let private_key = Secp256k1Crypto::generate_private_key().unwrap();
    let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"test message";

    let signature = Secp256k1Crypto::sign(message, &private_key).unwrap();
    let is_valid = Secp256k1Crypto::verify(message, &signature, &public_key).unwrap();

    assert!(is_valid);
}

/// C# (.NET ECDsa / BouncyCastle) accepts high-s secp256k1 signatures; the
/// libsecp256k1 binding rejects them unless normalized. Verify the high-s
/// malleated form (s' = N - s) still validates, matching C# (consensus parity
/// for CryptoLib.verifyWithECDsa secp256k1 and Notary signature checks).
#[test]
fn secp256k1_verify_accepts_high_s_like_csharp() {
    use num_bigint::BigUint;

    let private_key = Secp256k1Crypto::generate_private_key().unwrap();
    let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"high-s parity";

    // The secp256k1 crate emits a canonical low-s signature.
    let low_sig = Secp256k1Crypto::sign(message, &private_key).unwrap();
    assert!(Secp256k1Crypto::verify(message, &low_sig, &public_key).unwrap());

    // Malleate to the high-s representative: s' = N - s (> N/2).
    let n = BigUint::parse_bytes(
        b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141",
        16,
    )
    .unwrap();
    let s = BigUint::from_bytes_be(&low_sig[32..]);
    let high_s = &n - &s;
    assert_ne!(high_s, s, "freshly-signed s must be low-s");

    let mut high_sig = [0u8; 64];
    high_sig[..32].copy_from_slice(&low_sig[..32]);
    let high_s_bytes = high_s.to_bytes_be();
    high_sig[64 - high_s_bytes.len()..].copy_from_slice(&high_s_bytes);

    assert!(
        Secp256k1Crypto::verify(message, &high_sig, &public_key).unwrap(),
        "high-s signature must verify (C# parity)"
    );
}

#[test]
fn secp256r1_prehash_signs_keccak_digest() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"prehash payload";
    let digest = Crypto::keccak256(message);

    let signature = Secp256r1Crypto::sign_prehash(&digest, &private_key).unwrap();

    assert!(
        super::verify_ecdsa_raw64_with_hash(
            message,
            &signature,
            &public_key,
            ECCurve::Secp256r1,
            HashAlgorithm::Keccak256,
        )
        .unwrap()
    );
}

#[test]
fn neofs_p256_sha512_signs_and_verifies() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"neofs bearer token";

    let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

    assert_eq!(signature.len(), 65);
    assert_eq!(signature[0], NEOFS_ECDSA_SHA512_PREFIX);
    assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
}

#[test]
fn neofs_p256_sha512_rejects_mutated_inputs() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"neofs bearer token";
    let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

    assert!(
        !Secp256r1Crypto::verify_neofs_sha512(b"different message", &signature, &public_key)
            .unwrap()
    );

    let mut mutated = signature;
    mutated[64] ^= 0x01;
    assert!(!Secp256r1Crypto::verify_neofs_sha512(message, &mutated, &public_key).unwrap());

    assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature[..64], &public_key).is_err());
    assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &[0x02]).is_err());
}

#[test]
fn neofs_p256_sha512_preserves_ignored_prefix_behavior() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"neofs bearer token";
    let mut signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();
    signature[0] = 0xff;

    assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
}

#[test]
fn neofs_p256_sha512_rejects_regular_p256_signature() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"neofs bearer token";
    let signature = Secp256r1Crypto::sign(message, &private_key).unwrap();
    let mut neofs_shaped_signature = [0u8; 65];
    neofs_shaped_signature[0] = NEOFS_ECDSA_SHA512_PREFIX;
    neofs_shaped_signature[1..].copy_from_slice(&signature);

    assert!(
        !Secp256r1Crypto::verify_neofs_sha512(message, &neofs_shaped_signature, &public_key)
            .unwrap()
    );
}

#[test]
fn recover_public_key_round_trips_and_rejects_bad_input() {
    use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};

    let secp = Secp256k1::new();
    let sk = SecretKey::from_slice(&[0x11u8; 32]).unwrap();
    let expected = PublicKey::from_secret_key(&secp, &sk).serialize().to_vec();
    let message_hash = [0x42u8; 32];
    let msg = Message::from_digest_slice(&message_hash).unwrap();

    let (rec_id, compact) = secp.sign_ecdsa_recoverable(&msg, &sk).serialize_compact();
    let v = rec_id.to_i32() as u8;

    // 65-byte r||s||v with raw recovery id (0..3) recovers the signer key.
    let mut sig = compact.to_vec();
    sig.push(v);
    assert_eq!(
        Secp256k1Crypto::recover_public_key(&message_hash, &sig).unwrap(),
        expected
    );

    // Ethereum-style v (27/28) is normalized to the same recovery id.
    let mut sig_eth = compact.to_vec();
    sig_eth.push(v + 27);
    assert_eq!(
        Secp256k1Crypto::recover_public_key(&message_hash, &sig_eth).unwrap(),
        expected
    );

    // 64-byte EIP-2098 compact form (yParity fused into s) also recovers.
    let mut sig_compact = compact.to_vec();
    if v & 1 == 1 {
        sig_compact[32] |= 0x80;
    }
    assert_eq!(
        Secp256k1Crypto::recover_public_key(&message_hash, &sig_compact).unwrap(),
        expected
    );

    // Wrong-length hash or signature is an error (C# RecoverSecp256K1 -> null).
    assert!(Secp256k1Crypto::recover_public_key(&[0u8; 31], &sig).is_err());
    assert!(Secp256k1Crypto::recover_public_key(&message_hash, &sig[..63]).is_err());
}

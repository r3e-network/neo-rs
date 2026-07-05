use super::*;
use ed25519_dalek::Signer as _;

fn scalar(value: u8) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[31] = value;
    bytes
}

/// C# `Crypto.VerifySignature` (and thus `CryptoLib.verifyWithECDsa`) accepts
/// both low-s and high-s secp256k1 signatures. RustCrypto's k256 verifier
/// enforces low-s, so the raw verify paths must normalize first to keep
/// contract-execution parity (a high-s signature must not fault where C#
/// returns true). secp256r1/p256 already accepts high-s, so only k256 needs it.
#[test]
fn secp256k1_verify_paths_accept_high_s_like_csharp() {
    use crate::Secp256k1Crypto;
    let private_key = [9u8; 32];
    let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
    let message = b"high-s parity";
    let low_sig = Secp256k1Crypto::sign(message, &private_key).unwrap();
    // s' = n - s (secp256k1 order) yields the malleable high-s twin.
    let n = num_bigint::BigUint::parse_bytes(
        b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141",
        16,
    )
    .unwrap();
    let s = num_bigint::BigUint::from_bytes_be(&low_sig[32..]);
    let high_s = (&n - &s).to_bytes_be();
    let mut high_sig = low_sig;
    let mut padded = [0u8; 32];
    padded[32 - high_s.len()..].copy_from_slice(&high_s);
    high_sig[32..].copy_from_slice(&padded);

    assert!(EcdsaVerify::verify_signature_secp256k1(&public_key, message, &high_sig).unwrap());
    assert!(
        EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256k1,
            &public_key,
            message,
            &high_sig,
            HashAlgorithm::Sha256,
        )
        .unwrap()
    );
}

fn p256_signing_key() -> P256SigningKey {
    P256SigningKey::from_bytes(&p256::FieldBytes::from(scalar(1))).unwrap()
}

fn k256_signing_key() -> K256SigningKey {
    K256SigningKey::from_bytes(&k256::FieldBytes::from(scalar(2))).unwrap()
}

fn ed25519_signing_key() -> Ed25519SigningKey {
    Ed25519SigningKey::from_bytes(&[7u8; 32])
}

#[test]
fn test_ec_curve_sizes() {
    assert_eq!(ECCurve::Secp256r1.compressed_size(), 33);
    assert_eq!(ECCurve::Secp256k1.compressed_size(), 33);
    assert_eq!(ECCurve::Ed25519.compressed_size(), 32);

    assert_eq!(ECCurve::Secp256r1.uncompressed_size(), 65);
    assert_eq!(ECCurve::Secp256k1.uncompressed_size(), 65);
    assert_eq!(ECCurve::Ed25519.uncompressed_size(), 32);
}

#[test]
fn test_ec_point_creation() {
    let signing_key = p256_signing_key();
    let pub_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let point = ECPoint::new(ECCurve::Secp256r1, pub_bytes.clone()).unwrap();
    assert_eq!(point.curve(), ECCurve::Secp256r1);
    assert_eq!(point.as_bytes(), pub_bytes.as_slice());
    assert!(point.is_on_curve());
}

#[test]
fn test_ec_point_invalid_prefix() {
    let mut data = vec![0x04]; // Invalid prefix for compressed
    data.extend_from_slice(&[0xAA; 32]);
    let result = ECPoint::new(ECCurve::Secp256r1, data);
    assert!(result.is_err());
}

#[test]
fn test_ec_point_invalid_size() {
    let data = vec![0x02; 20]; // Wrong size
    let result = ECPoint::new(ECCurve::Secp256r1, data);
    assert!(result.is_err());
}

#[test]
fn test_ec_point_infinity() {
    let infinity = ECPoint::infinity(ECCurve::Secp256r1);
    assert!(infinity.is_infinity());
    assert!(!infinity.is_on_curve());
}

#[test]
fn test_ec_point_decode_compressed() {
    let signing_key = p256_signing_key();
    let compressed = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let point = ECPoint::from_bytes(&compressed).unwrap();
    assert_eq!(point.curve(), ECCurve::Secp256r1);
    assert!(point.is_on_curve());
}

#[test]
fn test_ec_point_from_uncompressed() {
    let signing_key = p256_signing_key();
    let uncompressed = signing_key
        .verifying_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    let point = ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, &uncompressed).unwrap();
    assert_eq!(point.as_bytes().len(), ECCurve::Secp256r1.compressed_size());
    assert!(point.is_on_curve());
}

#[test]
fn test_ec_point_ordering() {
    let first = p256_signing_key();
    let second = P256SigningKey::from_bytes(&p256::FieldBytes::from(scalar(3))).unwrap();

    let point1 = ECPoint::new(
        ECCurve::Secp256r1,
        first
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec(),
    )
    .unwrap();
    let point2 = ECPoint::new(
        ECCurve::Secp256r1,
        second
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec(),
    )
    .unwrap();

    assert_ne!(point1, point2);
    assert_eq!(
        point1.cmp(&point2),
        point1.as_bytes().cmp(point2.as_bytes())
    );
}

#[test]
fn test_verify_secp256r1_signature() {
    let signing_key = p256_signing_key();
    let pub_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let message = b"neo-secp256r1";
    let signature: P256Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    assert!(
        EcdsaVerify::verify_signature_secp256r1(&pub_bytes, message, signature_bytes.as_slice())
            .unwrap()
    );

    let mut bad_sig = signature_bytes;
    bad_sig[0] ^= 0x01;
    assert!(!EcdsaVerify::verify_signature_secp256r1(&pub_bytes, message, &bad_sig).unwrap());
}

#[test]
fn verify_with_hash_sha256_matches_default_verify() {
    // For SHA-256, verify_signature_with_hash must agree with the existing
    // SHA-256 ECDSA verify (whose Verifier::verify hashes with SHA-256).
    let signing_key = p256_signing_key();
    let pub_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let message = b"neo-ecdsa-hash-selectable";
    let signature: P256Signature = signing_key.sign(message);
    let sig_bytes = signature.to_bytes();

    assert!(
        EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256r1,
            &pub_bytes,
            message,
            sig_bytes.as_slice(),
            HashAlgorithm::Sha256,
        )
        .unwrap()
    );
    assert!(
        EcdsaVerify::verify_signature_secp256r1(&pub_bytes, message, sig_bytes.as_slice()).unwrap()
    );

    // A Keccak-256 verification of a SHA-256 signature must fail (the digest
    // differs), and a malformed key yields false (not an error).
    assert!(
        !EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256r1,
            &pub_bytes,
            message,
            sig_bytes.as_slice(),
            HashAlgorithm::Keccak256,
        )
        .unwrap()
    );
    assert!(
        !EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256r1,
            &[0u8; 33],
            message,
            sig_bytes.as_slice(),
            HashAlgorithm::Sha256,
        )
        .unwrap()
    );
}

#[test]
fn verify_with_hash_keccak256_round_trips_both_curves() {
    use k256::ecdsa::signature::hazmat::PrehashSigner as K256PrehashSigner;
    use sha3::{Digest as _, Keccak256};

    let message = b"neo-keccak-ecdsa";
    let digest = Keccak256::digest(message);

    // secp256r1 + Keccak-256.
    let p256_key = p256_signing_key();
    let p256_pub = p256_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let p256_sig: P256Signature = p256_key.sign_prehash(&digest).unwrap();
    assert!(
        EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256r1,
            &p256_pub,
            message,
            p256_sig.to_bytes().as_slice(),
            HashAlgorithm::Keccak256,
        )
        .unwrap()
    );
    // The same signature must NOT verify under SHA-256 (wrong digest).
    assert!(
        !EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256r1,
            &p256_pub,
            message,
            p256_sig.to_bytes().as_slice(),
            HashAlgorithm::Sha256,
        )
        .unwrap()
    );

    // secp256k1 + Keccak-256.
    let k256_key = k256_signing_key();
    let k256_pub = k256_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let k256_sig: K256Signature = k256_key.sign_prehash(&digest).unwrap();
    assert!(
        EcdsaVerify::verify_signature_with_hash(
            ECCurve::Secp256k1,
            &k256_pub,
            message,
            k256_sig.to_bytes().as_slice(),
            HashAlgorithm::Keccak256,
        )
        .unwrap()
    );
}

#[test]
fn test_verify_secp256k1_signature() {
    let signing_key = k256_signing_key();
    let pub_bytes = signing_key
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let message = b"neo-secp256k1";
    let signature: K256Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    assert!(
        EcdsaVerify::verify_signature_secp256k1(&pub_bytes, message, signature_bytes.as_slice())
            .unwrap()
    );

    let mut bad_sig = signature_bytes;
    bad_sig[0] ^= 0x01;
    assert!(!EcdsaVerify::verify_signature_secp256k1(&pub_bytes, message, &bad_sig).unwrap());
}

#[test]
fn test_verify_ed25519_signature() {
    let signing_key = ed25519_signing_key();
    let verifying_key = signing_key.verifying_key();
    let message = b"neo-ed25519";
    let signature = signing_key.sign(message);

    assert!(
        EcdsaVerify::verify_ed25519(&verifying_key.to_bytes(), message, &signature.to_bytes())
            .unwrap()
    );

    let mut bad_sig = signature.to_bytes();
    bad_sig[0] ^= 0x01;
    assert!(!EcdsaVerify::verify_ed25519(&verifying_key.to_bytes(), message, &bad_sig).unwrap());
}

/// Adversarial-input parity vectors for `verify_ed25519`, asserting the verdict
/// matches C# `CryptoLib.VerifyWithEd25519` (BouncyCastle `Ed25519Signer`).
///
/// Every `expected` value below was produced by running the exact BouncyCastle
/// verification path Neo uses (`Ed25519Signer.Init(false, Ed25519PublicKeyParameters)`
/// + `VerifySignature`) on the same `(pubkey, msg, sig)` triple. BouncyCastle
/// uses the **cofactorless** RFC 8032 equation and:
///   * rejects a small-order public key A (`lowOrderA` -> false),
///   * ACCEPTS a small-order R (`lowOrderR` -> true) — the case where the old
///     `verify_strict` FAULTED, diverging from C#,
///   * rejects a non-canonical scalar S >= L (`noncanonical_S` -> false),
///   * accepts a well-formed valid signature (`valid` -> true).
#[test]
fn verify_ed25519_matches_bouncycastle_accept_set() {
    // (label, pubkey_hex, sig_hex, msg_ascii, expected)
    let cases: &[(&str, &str, &str, &[u8], bool)] = &[
        // Valid signature over "neo" with a real key: both C# and neo-rs accept.
        (
            "valid_baseline",
            "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
            "df286c9498ea5a3f85ea8ba88f12384f4149cb9333ffd83d1780b6fd5b1adc6bd031cc295c29c83de73a2413aefd59a831278c674176bb2cd2b8243850407304",
            b"neo",
            true,
        ),
        // Small-order public key A (an order-8 torsion point), with a signature
        // that satisfies the cofactorless equation. BouncyCastle
        // `CheckPointFullVar` rejects a small-order A -> false. Our fix keeps
        // this rejection via `is_weak`.
        (
            "lowOrderA",
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
            "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc050000000000000000000000000000000000000000000000000000000000000000",
            b"la1",
            false,
        ),
        // Small-order R (an order-8 torsion point) with a full-order A that
        // carries a torsion component; the signature satisfies the cofactorless
        // equation. BouncyCastle ACCEPTS this (it only checks R's canonical
        // encoding, not its order) -> true. `verify_strict` would FAULT here,
        // which is the exact consensus divergence this fix removes.
        (
            "lowOrderR",
            "c6e2cb790d0e8833a455b24cc304bf11cc0e2d0b6625c64663aa9ee64506188d",
            "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa04f5ec52d9c7966bc348e298fd98ecf7669b0199bad90440d1de6905588d920e",
            b"lor0",
            true,
        ),
        // Non-canonical scalar S = S0 + L (>= the group order L). BouncyCastle
        // `Scalar25519.CheckVar` rejects S >= L -> false. ed25519-dalek's
        // `check_scalar` (`Scalar::from_canonical_bytes`) enforces the same bound.
        (
            "noncanonical_S_geq_L",
            "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
            "df286c9498ea5a3f85ea8ba88f12384f4149cb9333ffd83d1780b6fd5b1adc6bbd05c286768cda95bdd71bb68cf738bd31278c674176bb2cd2b8243850407314",
            b"neo",
            false,
        ),
        // A valid signature verified against the wrong message: false.
        (
            "wrong_msg",
            "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664",
            "df286c9498ea5a3f85ea8ba88f12384f4149cb9333ffd83d1780b6fd5b1adc6bd031cc295c29c83de73a2413aefd59a831278c674176bb2cd2b8243850407304",
            b"NEO",
            false,
        ),
    ];

    for (label, pub_hex, sig_hex, msg, expected) in cases {
        let pubkey = hex::decode(pub_hex).unwrap();
        let sig = hex::decode(sig_hex).unwrap();
        let got = EcdsaVerify::verify_ed25519(&pubkey, msg, &sig).unwrap_or(false);
        assert_eq!(
            got, *expected,
            "verify_ed25519 disagrees with BouncyCastle on case '{label}'"
        );

        // The higher-level `Ed25519Crypto::verify` (used by the generic ECDsa
        // dispatch) must reach the same verdict for 32/64-byte inputs.
        if pubkey.len() == 32 && sig.len() == 64 {
            let pk: [u8; 32] = pubkey.clone().try_into().unwrap();
            let sg: [u8; 64] = sig.clone().try_into().unwrap();
            let got2 = crate::Ed25519Crypto::verify(msg, &sg, &pk).unwrap_or(false);
            assert_eq!(
                got2, *expected,
                "Ed25519Crypto::verify disagrees with BouncyCastle on case '{label}'"
            );
        }
    }
}

#[test]
fn test_generate_keypair_roundtrip() {
    let (private_key, public_point) = EcdsaVerify::generate_keypair(ECCurve::Secp256r1).unwrap();
    assert_eq!(
        public_point.as_bytes().len(),
        ECCurve::Secp256r1.compressed_size()
    );

    let private_array: [u8; 32] = private_key.as_slice().try_into().unwrap();
    let signing_key = P256SigningKey::from_bytes(&p256::FieldBytes::from(private_array)).unwrap();
    let message = b"keygen-roundtrip";
    let signature: P256Signature = signing_key.sign(message);
    let signature_bytes = signature.to_bytes();

    assert!(
        EcdsaVerify::verify_signature(
            ECCurve::Secp256r1,
            public_point.as_bytes(),
            message,
            signature_bytes.as_slice()
        )
        .unwrap()
    );
}

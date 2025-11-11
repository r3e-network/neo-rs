use super::{sign_with_algorithm, Secp256r1Sign, Secp256r1Verify};
use crate::{ecc256, hash_algorithm::HashAlgorithm};
use hex_literal::hex;
use p256::{ecdsa::SigningKey, SecretKey};

#[test]
fn sign_and_verify() {
    let sk_bytes = hex!("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b848a7d84b8b620");
    let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
    let secret = SecretKey::from_slice(&sk_bytes).unwrap();
    let signing = SigningKey::from(secret.clone());
    let public_encoded = signing.verifying_key().to_encoded_point(true);
    let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();
    let message = b"neo-n3";

    let signature = private.secp256r1_sign(message).unwrap();
    public.secp256r1_verify(message, &signature).unwrap();
}

#[test]
fn sign_with_keccak_roundtrip() {
    let sk_bytes = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
    let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
    let secret = SecretKey::from_slice(&sk_bytes).unwrap();
    let public_encoded = SigningKey::from(secret)
        .verifying_key()
        .to_encoded_point(true);
    let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();

    let signature =
        sign_with_algorithm(&private, b"keccak payload", HashAlgorithm::Keccak256).unwrap();
    super::verify_with_algorithm(
        &public,
        b"keccak payload",
        &signature,
        HashAlgorithm::Keccak256,
    )
    .unwrap();
}

#[test]
fn sign_with_sha512_roundtrip() {
    let sk_bytes = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
    let private = ecc256::PrivateKey::from_slice(&sk_bytes).unwrap();
    let secret = SecretKey::from_slice(&sk_bytes).unwrap();
    let public_encoded = SigningKey::from(secret)
        .verifying_key()
        .to_encoded_point(true);
    let public = ecc256::PublicKey::from_sec1_bytes(public_encoded.as_bytes()).unwrap();

    let signature =
        sign_with_algorithm(&private, b"sha512 payload", HashAlgorithm::Sha512).unwrap();
    super::verify_with_algorithm(
        &public,
        b"sha512 payload",
        &signature,
        HashAlgorithm::Sha512,
    )
    .unwrap();
}

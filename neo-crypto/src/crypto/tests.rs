use super::{sign, verify, Curve};
use crate::{ecc256::Keypair, ecc256::PrivateKey, hash_algorithm::HashAlgorithm};
use hex_literal::hex;

#[test]
fn sign_verify_p256_roundtrip() {
    let private = hex!("c37b8b0c7c0b8c1fe4f602c3f0f2f3536bc3a1ad9ecf15ff86f9fee9b7dd2f75");
    let p256 = PrivateKey::from_slice(&private).unwrap();
    let keypair = Keypair::from_private(p256.clone()).unwrap();
    let public = keypair.public_key.to_compressed();

    let sig = sign(b"neo", &private, Curve::Secp256r1, HashAlgorithm::Sha256).unwrap();
    verify(
        b"neo",
        &sig,
        &public,
        Curve::Secp256r1,
        HashAlgorithm::Sha256,
    )
    .unwrap();
}

#[test]
fn sign_verify_k1_roundtrip() {
    use ::secp256k1::{PublicKey as K1PublicKey, Secp256k1 as K1Ctx, SecretKey as K1SecretKey};

    let private = hex!("1b7f730fc3ac386a1ae1c2cbaabdd99e3bb85da7d5236f9b1a92bb0b742d30ca");
    let secp = K1Ctx::new();
    let secret = K1SecretKey::from_slice(&private).expect("valid secp256k1 private key");
    let public = K1PublicKey::from_secret_key(&secp, &secret).serialize();

    let sig = sign(b"neo", &private, Curve::Secp256k1, HashAlgorithm::Sha256).unwrap();
    verify(
        b"neo",
        &sig,
        &public,
        Curve::Secp256k1,
        HashAlgorithm::Sha256,
    )
    .unwrap();
}

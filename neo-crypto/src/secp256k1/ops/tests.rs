use super::*;
use crate::hash_algorithm::HashAlgorithm;
use hex_literal::hex;
use secp256k1::Secp256k1;

#[test]
fn sign_and_verify_roundtrip() {
    let private = hex!("1b7f730fc3ac386a1ae1c2cbaabdd99e3bb85da7d5236f9b1a92bb0b742d30ca");
    let secp = Secp256k1::new();
    let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
    let public = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
    let signature = sign(b"neo-k1", &private, HashAlgorithm::Sha256).unwrap();
    verify(b"neo-k1", &signature, &public, HashAlgorithm::Sha256).unwrap();
}

#[test]
fn supports_keccak_hashing() {
    let private = hex!("38b28fe5602eb700f8502e2c166b03db6602bc917e35a31995f1b2d287f4d137");
    let secp = Secp256k1::new();
    let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
    let public = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
    let signature = sign(b"neo-keccak", &private, HashAlgorithm::Keccak256).unwrap();
    verify(b"neo-keccak", &signature, &public, HashAlgorithm::Keccak256).unwrap();
}

#[test]
fn recover_public_key_from_recoverable_signature() {
    let private = hex!("6d16ca2b9f10f8917ac12f90b91f864b0db1d0545d142e9d5b75f1c83c5f4321");
    let secp = Secp256k1::new();
    let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
    let expected = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
    let (sig, rec) = sign_recoverable(b"recover", &private, HashAlgorithm::Sha256).expect("sign");
    let recovered =
        recover_public_key(b"recover", &sig, rec, HashAlgorithm::Sha256).expect("recover");
    assert_eq!(expected.to_vec(), recovered);
}

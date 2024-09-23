extern crate secp256k1;
extern crate sha2;
extern crate num_bigint;
extern crate num_traits;
extern crate assert;
extern crate test_case;

use secp256k1::{Secp256k1, SecretKey, PublicKey, Message, Signature};
use sha2::{Sha256, Digest};
use num_bigint::BigInt;
use assert::{assert_eq, assert_ne, assert_err, assert_ok};
use test_case::test_case;

use crate::crypto::keys::{NewPrivateKey, NewSecp256k1PrivateKey};

#[test]
fn test_issue1223() {
    let d = BigInt::parse_bytes(b"75066030006596498716801752450216843918658392116070031536027203512060270094427", 10).unwrap();
    let x = BigInt::parse_bytes(b"56810139335762307690884151098712528235297095596167964448512639328424930082240", 10).unwrap();
    let y = BigInt::parse_bytes(b"108055740278314806025442297642651169427004858252141003070998851291610422839293", 10).unwrap();

    let secp = Secp256k1::new();
    let private_key = SecretKey::from_slice(&d.to_bytes_be().1).unwrap();
    let public_key = PublicKey::from_secret_key(&secp, &private_key);

    let mut hasher = Sha256::new();
    hasher.update(b"sample");
    let hashed_data = hasher.finalize();

    let message = Message::from_slice(&hashed_data).unwrap();
    let signature = secp.sign(&message, &private_key);

    assert!(secp.verify(&message, &signature, &public_key).is_ok());
}

#[test]
fn test_pub_key_verify() {
    let data = b"sample";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hashed_data = hasher.finalize();

    #[test_case]
    fn secp256r1() {
        let priv_key = NewPrivateKey().unwrap();
        let signed_data = priv_key.sign(data);
        let pub_key = priv_key.public_key();
        let result = pub_key.verify(&signed_data, &hashed_data);
        let expected = true;
        assert_eq!(expected, result);

        // Small signature, no panic.
        assert!(!pub_key.verify(&[1, 2, 3], &hashed_data));

        let pub_key = PublicKey::default();
        assert!(!pub_key.verify(&signed_data, &hashed_data));
    }

    #[test_case]
    fn secp256k1() {
        let priv_key = NewSecp256k1PrivateKey().unwrap();
        let signed_data = priv_key.sign_hash(&hashed_data);
        let pub_key = priv_key.public_key();
        assert!(pub_key.verify(&signed_data, &hashed_data));

        let pub_key = PublicKey::default();
        assert!(!pub_key.verify(&signed_data, &hashed_data));
    }
}

#[test]
fn test_wrong_pub_key() {
    let sample = b"sample";
    let mut hasher = Sha256::new();
    hasher.update(sample);
    let hashed_data = hasher.finalize();

    #[test_case]
    fn secp256r1() {
        let priv_key = NewPrivateKey().unwrap();
        let signed_data = priv_key.sign(sample);

        let second_priv_key = NewPrivateKey().unwrap();
        let wrong_pub_key = second_priv_key.public_key();

        let actual = wrong_pub_key.verify(&signed_data, &hashed_data);
        let expected = false;
        assert_eq!(expected, actual);
    }

    #[test_case]
    fn secp256k1() {
        let priv_key = NewSecp256k1PrivateKey().unwrap();
        let signed_data = priv_key.sign_hash(&hashed_data);

        let second_priv_key = NewSecp256k1PrivateKey().unwrap();
        let wrong_pub_key = second_priv_key.public_key();

        assert!(!wrong_pub_key.verify(&signed_data, &hashed_data));
    }
}

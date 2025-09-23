use hex::decode;
// Removed neo_bls12_381 and neo_cryptography dependencies - using external crypto crates directly
use neo_core::UInt160;
use neo_core::crypto_utils::{NeoHash, Secp256k1Crypto, Secp256r1Crypto, Ed25519Crypto, Bls12381Crypto};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::{CryptoLib, NativeContract, TriggerType};
use std::convert::TryInto;

fn new_engine() -> ApplicationEngine {
    ApplicationEngine::new(TriggerType::Application, 10_000_000)
}

fn run_contract(method: &str, args: Vec<Vec<u8>>) -> Vec<u8> {
    run_contract_result(method, args).expect("contract call succeeded")
}

fn run_contract_result(method: &str, args: Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut engine = new_engine();
    let contract = CryptoLib::default();
    let native: &dyn NativeContract = &contract;

    native
        .invoke(&mut engine, method, &args)
        .map_err(|err| err.to_string())
}

#[test]
fn crypto_contract_hash_matches_reference() {
    let contract = CryptoLib::default();
    let expected = UInt160::from_bytes(&[
        0x72, 0x6c, 0xb6, 0xe0, 0xcd, 0x86, 0x28, 0xa1, 0x35, 0x0a, 0x61, 0x13, 0x84, 0x68, 0x89,
        0x11, 0xab, 0x75, 0xf5, 0x1b,
    ])
    .expect("valid hash");

    assert_eq!(contract.hash(), expected);
}

#[test]
fn crypto_sha256_matches_reference() {
    let data = b"neo-smart-contract";

    let expected = Crypto::sha256(data);
    let result = run_contract("sha256", vec![data.to_vec()]);

    assert_eq!(result, expected);
}

#[test]
fn crypto_ripemd160_matches_reference() {
    let data = b"neo-smart-contract";

    let expected = Crypto::ripemd160(data);
    let result = run_contract("ripemd160", vec![data.to_vec()]);

    assert_eq!(result, expected);
}

fn hex32(hex: &str) -> [u8; 32] {
    let bytes = decode(hex).expect("valid hex");
    bytes.try_into().expect("32-byte key")
}

fn secp256r1_private_key_one() -> [u8; 32] {
    hex32("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721")
}

fn secp256r1_private_key_two() -> [u8; 32] {
    hex32("1e99423a4ed27608a15a2616cf8f81b0158906b3529438661d2b8a0cdb1c6ce9")
}

fn secp256k1_private_key_one() -> [u8; 32] {
    hex32("4c0883a69102937d6234147f1b886b5ef984d7454c5852b68d1f6f7f98c6d4c6")
}

fn secp256k1_private_key_two() -> [u8; 32] {
    hex32("bbf4b9f7a0e2f6d7c3a2b1e9f5d4c3b2a18765f1234567890fedcba987654321")
}

fn sign_p256(message: &[u8], private_key: &[u8; 32]) -> Vec<u8> {
    ECDsa::sign_neo_format(message, private_key)
        .expect("signature generation")
        .to_vec()
}

fn p256_public_key(private_key: &[u8; 32]) -> Vec<u8> {
    ECDsa::derive_compressed_public_key(private_key).expect("public key derivation")
}

fn sign_k256(message: &[u8], private_key: &[u8; 32]) -> Vec<u8> {
    ECDsa::sign_secp256k1(message, private_key).expect("signature generation")
}

fn k256_public_key(private_key: &[u8; 32]) -> Vec<u8> {
    helper::private_key_to_public_key(private_key).expect("public key derivation")
}

fn concat_bytes(parts: &[&[u8]]) -> Vec<u8> {
    let total: usize = parts.iter().map(|part| part.len()).sum();
    let mut combined = Vec::with_capacity(total);
    for part in parts {
        combined.extend_from_slice(part);
    }
    combined
}

fn g1_to_compressed(point: &G1Projective) -> Vec<u8> {
    point.to_affine().to_compressed().to_vec()
}

fn g1_to_uncompressed(point: &G1Projective) -> Vec<u8> {
    point.to_affine().to_uncompressed().to_vec()
}

fn g2_to_compressed(point: &G2Projective) -> Vec<u8> {
    point.to_affine().to_compressed().to_vec()
}

fn g2_to_uncompressed(point: &G2Projective) -> Vec<u8> {
    point.to_affine().to_uncompressed().to_vec()
}

fn scalar_from_u64(value: u64) -> Scalar {
    let mut bytes = [0u8; Scalar::SIZE];
    bytes[Scalar::SIZE - 8..].copy_from_slice(&value.to_be_bytes());
    Scalar::from_fixed_bytes(&bytes).expect("valid scalar")
}

fn scalar_to_bytes(value: u64) -> Vec<u8> {
    scalar_from_u64(value).to_bytes().to_vec()
}

#[test]
fn crypto_verify_with_ecdsa_secp256r1_accepts_valid_signature() {
    let private_key = secp256r1_private_key_one();
    let message = b"neo-smart-contract";
    let signature = sign_p256(message, &private_key);
    let public_key = p256_public_key(&private_key);

    let args = vec![message.to_vec(), signature, public_key];
    let result = run_contract("verifyWithECDsaSecp256r1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256r1_rejects_modified_message() {
    let private_key = secp256r1_private_key_one();
    let message = b"neo-smart-contract";
    let signature = sign_p256(message, &private_key);
    let public_key = p256_public_key(&private_key);

    let args = vec![b"neo-smart-contract!".to_vec(), signature, public_key];
    let result = run_contract("verifyWithECDsaSecp256r1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256k1_accepts_valid_signature() {
    let private_key = secp256k1_private_key_one();
    let message = b"neo-smart-contract";
    let signature = sign_k256(message, &private_key);
    let public_key = k256_public_key(&private_key);

    let args = vec![message.to_vec(), signature, public_key];
    let result = run_contract("verifyWithECDsaSecp256k1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256k1_rejects_modified_message() {
    let private_key = secp256k1_private_key_one();
    let message = b"neo-smart-contract";
    let signature = sign_k256(message, &private_key);
    let public_key = k256_public_key(&private_key);

    let args = vec![b"neo-smart-contract!".to_vec(), signature, public_key];
    let result = run_contract("verifyWithECDsaSecp256k1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_check_multisig_secp256r1_accepts_signatures() {
    let key_one = secp256r1_private_key_one();
    let key_two = secp256r1_private_key_two();
    let message = b"neo-multisig";

    let signature_one = sign_p256(message, &key_one);
    let signature_two = sign_p256(message, &key_two);

    let public_key_one = p256_public_key(&key_one);
    let public_key_two = p256_public_key(&key_two);

    let args = vec![
        message.to_vec(),
        concat_bytes(&[&signature_one, &signature_two]),
        concat_bytes(&[&public_key_one, &public_key_two]),
    ];

    let result = run_contract("checkMultisig", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_check_multisig_secp256k1_accepts_signatures() {
    let key_one = secp256k1_private_key_one();
    let key_two = secp256k1_private_key_two();
    let message = b"neo-multisig";

    let signature_one = sign_k256(message, &key_one);
    let signature_two = sign_k256(message, &key_two);

    let public_key_one = k256_public_key(&key_one);
    let public_key_two = k256_public_key(&key_two);

    let args = vec![
        message.to_vec(),
        concat_bytes(&[&signature_one, &signature_two]),
        concat_bytes(&[&public_key_one, &public_key_two]),
    ];

    let result = run_contract("checkMultisigWithECDsaSecp256k1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_check_multisig_secp256k1_rejects_tampered_signature() {
    let key_one = secp256k1_private_key_one();
    let key_two = secp256k1_private_key_two();
    let message = b"neo-multisig";

    let signature_one = sign_k256(message, &key_one);
    let mut signature_two = sign_k256(message, &key_two);
    signature_two[0] ^= 0xFF; // corrupt signature

    let public_key_one = k256_public_key(&key_one);
    let public_key_two = k256_public_key(&key_two);

    let args = vec![
        message.to_vec(),
        concat_bytes(&[&signature_one, &signature_two]),
        concat_bytes(&[&public_key_one, &public_key_two]),
    ];

    let result = run_contract("checkMultisigWithECDsaSecp256k1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_bls12381_add_matches_expected() {
    let point_a = G1Projective::generator();
    let point_b = point_a * scalar_from_u64(3);
    let expected = g1_to_compressed(&(point_a + point_b.clone()));

    let args = vec![g1_to_compressed(&point_a), g1_to_compressed(&point_b)];
    let result = run_contract("bls12381Add", args);

    assert_eq!(result, expected);
}

#[test]
fn crypto_bls12381_equal_matches_expected() {
    let point = G1Projective::generator();
    let equal_args = vec![g1_to_compressed(&point), g1_to_compressed(&point)];
    let equal = run_contract("bls12381Equal", equal_args);
    assert_eq!(equal, vec![1]);

    let different = g1_to_compressed(&(point * scalar_from_u64(2)));
    let unequal_args = vec![g1_to_compressed(&point), different];
    let unequal = run_contract("bls12381Equal", unequal_args);
    assert_eq!(unequal, vec![0]);
}

#[test]
fn crypto_bls12381_mul_matches_expected() {
    let point = G1Projective::generator();
    let scalar_value = 7u64;
    let expected = g1_to_compressed(&(point * scalar_from_u64(scalar_value)));

    let args = vec![
        g1_to_compressed(&point),
        scalar_to_bytes(scalar_value),
        vec![0u8],
    ];
    let result = run_contract("bls12381Mul", args);

    assert_eq!(result, expected);
}

#[test]
fn crypto_bls12381_pairing_matches_expected() {
    let g1 = G1Projective::generator();
    let g2 = G2Projective::generator();
    let result = run_contract(
        "bls12381Pairing",
        vec![g1_to_compressed(&g1), g2_to_compressed(&g2)],
    );

    let expected = pairing(&G1Affine::from(g1.clone()), &G2Affine::from(g2.clone()))
        .to_bytes()
        .to_vec();

    assert_eq!(result, expected);
}

#[test]
fn crypto_bls12381_pairing_accepts_uncompressed_inputs() {
    let g1 = G1Projective::generator();
    let g2 = G2Projective::generator();
    let result = run_contract(
        "bls12381Pairing",
        vec![g1_to_uncompressed(&g1), g2_to_uncompressed(&g2)],
    );

    let expected = pairing(&G1Affine::from(g1.clone()), &G2Affine::from(g2.clone()))
        .to_bytes()
        .to_vec();

    assert_eq!(result, expected);
}

#[test]
fn crypto_bls12381_pairing_rejects_missing_g2() {
    let g1 = G1Projective::generator();
    let error = run_contract_result("bls12381Pairing", vec![g1_to_compressed(&g1)])
        .expect_err("missing g2 must return error");
    assert!(
        error.contains("requires G1 and G2 point"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_pairing_rejects_invalid_g1_bytes() {
    let mut bad_g1 = g1_to_compressed(&G1Projective::generator());
    bad_g1.reverse();
    let g2 = g2_to_compressed(&G2Projective::generator());
    let error = run_contract_result("bls12381Pairing", vec![bad_g1, g2])
        .expect_err("invalid g1 should error");
    assert!(
        error.contains("Invalid G1 point"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_pairing_rejects_invalid_g2_bytes() {
    let g1 = g1_to_compressed(&G1Projective::generator());
    let mut bad_g2 = g2_to_compressed(&G2Projective::generator());
    bad_g2.reverse();
    let error = run_contract_result("bls12381Pairing", vec![g1, bad_g2])
        .expect_err("invalid g2 should error");
    assert!(
        error.contains("Invalid G2 point"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_pairing_rejects_truncated_payload() {
    let mut g1 = g1_to_compressed(&G1Projective::generator());
    g1.pop();
    let g2 = g2_to_compressed(&G2Projective::generator());
    let error = run_contract_result("bls12381Pairing", vec![g1, g2])
        .expect_err("truncated g1 should error");
    assert!(
        error.contains("Unsupported BLS12-381 encoding length"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_serialize_round_trips_g1_and_g2() {
    let g1 = G1Projective::generator();
    let g2 = G2Projective::generator();

    let serialized_g1 = run_contract("bls12381Serialize", vec![g1_to_compressed(&g1)]);
    assert_eq!(serialized_g1, g1_to_compressed(&g1));

    let serialized_g2 = run_contract("bls12381Serialize", vec![g2_to_compressed(&g2)]);
    assert_eq!(serialized_g2, g2_to_compressed(&g2));
}

#[test]
fn crypto_bls12381_deserialize_rejects_invalid_length() {
    let error = run_contract_result("bls12381Deserialize", vec![vec![0u8; 10]])
        .expect_err("invalid length must return error");
    assert!(
        error.contains("Unsupported BLS12-381 encoding length"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_deserialize_rejects_g1_with_bad_flag() {
    let mut bytes = g1_to_compressed(&G1Projective::generator());
    bytes[0] ^= 0x80; // flip compression flag to make it invalid
    let error = run_contract_result("bls12381Deserialize", vec![bytes])
        .expect_err("invalid flag must return error");
    assert!(
        error.contains("Invalid G1 point"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_deserialize_rejects_g2_with_bad_flag() {
    let mut bytes = g2_to_compressed(&G2Projective::generator());
    bytes[0] ^= 0x80;
    let error = run_contract_result("bls12381Deserialize", vec![bytes])
        .expect_err("invalid G2 flag must return error");
    assert!(
        error.contains("Invalid G2 point"),
        "unexpected error: {error}"
    );
}

#[test]
fn crypto_bls12381_deserialize_rejects_g2_invalid_length() {
    let mut bytes = g2_to_compressed(&G2Projective::generator());
    bytes.pop(); // shorten to invalid length
    let error = run_contract_result("bls12381Deserialize", vec![bytes])
        .expect_err("invalid G2 length must return error");
    assert!(
        error.contains("Unsupported BLS12-381 encoding length"),
        "unexpected error: {error}"
    );
}

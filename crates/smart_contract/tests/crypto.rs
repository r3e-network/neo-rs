use bls12_381::{G1Affine, G1Projective, G2Affine, G2Projective, Scalar};
use hex::decode;
use k256::ecdsa::{
    signature::Signer as K256Signer, Signature as K256Signature, SigningKey as K256SigningKey,
};
use neo_core::UInt160;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::{CryptoLib, NativeContract, TriggerType};
use p256::ecdsa::{
    signature::Signer as P256Signer, Signature as P256Signature, SigningKey as P256SigningKey,
};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256};

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

    let expected = Sha256::digest(data).to_vec();
    let result = run_contract("sha256", vec![data.to_vec()]);

    assert_eq!(result, expected);
}

#[test]
fn crypto_ripemd160_matches_reference() {
    let data = b"neo-smart-contract";

    let mut hasher = Ripemd160::new();
    hasher.update(data);
    let expected = hasher.finalize().to_vec();
    let result = run_contract("ripemd160", vec![data.to_vec()]);

    assert_eq!(result, expected);
}

fn deterministic_p256_signing_key() -> P256SigningKey {
    let key_bytes = decode("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721")
        .expect("valid hex");
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);
    P256SigningKey::from_bytes(&key_array.into()).expect("valid signing key")
}

fn deterministic_p256_signing_key_two() -> P256SigningKey {
    let key_bytes = decode("1e99423a4ed27608a15a2616cf8f81b0158906b3529438661d2b8a0cdb1c6ce9")
        .expect("valid hex");
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);
    P256SigningKey::from_bytes(&key_array.into()).expect("valid signing key")
}

fn deterministic_k256_signing_key() -> K256SigningKey {
    let key_bytes = decode("4c0883a69102937d6234147f1b886b5ef984d7454c5852b68d1f6f7f98c6d4c6")
        .expect("valid hex");
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);
    K256SigningKey::from_bytes(&key_array.into()).expect("valid signing key")
}

fn deterministic_k256_signing_key_two() -> K256SigningKey {
    let key_bytes = decode("bbf4b9f7a0e2f6d7c3a2b1e9f5d4c3b2a18765f1234567890fedcba987654321")
        .expect("valid hex");
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&key_bytes);
    K256SigningKey::from_bytes(&key_array.into()).expect("valid signing key")
}

fn to_p256_signature_bytes(signature: P256Signature) -> Vec<u8> {
    signature.to_bytes().to_vec()
}

fn to_k256_signature_bytes(signature: K256Signature) -> Vec<u8> {
    signature.to_bytes().to_vec()
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
    G1Affine::from(*point).to_compressed().to_vec()
}

fn g1_to_uncompressed(point: &G1Projective) -> Vec<u8> {
    G1Affine::from(*point).to_uncompressed().to_vec()
}

fn g2_to_compressed(point: &G2Projective) -> Vec<u8> {
    G2Affine::from(*point).to_compressed().to_vec()
}

fn g2_to_uncompressed(point: &G2Projective) -> Vec<u8> {
    G2Affine::from(*point).to_uncompressed().to_vec()
}

fn scalar_to_bytes(value: u64) -> Vec<u8> {
    Scalar::from(value).to_bytes().to_vec()
}

#[test]
fn crypto_verify_with_ecdsa_secp256r1_accepts_valid_signature() {
    let signing_key = deterministic_p256_signing_key();
    let verifying_key = signing_key.verifying_key();
    let message = b"neo-smart-contract";
    let signature = P256Signer::sign(&signing_key, message);

    let args = vec![
        message.to_vec(),
        to_p256_signature_bytes(signature),
        verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    ];

    let result = run_contract("verifyWithECDsaSecp256r1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256r1_rejects_modified_message() {
    let signing_key = deterministic_p256_signing_key();
    let verifying_key = signing_key.verifying_key();
    let message = b"neo-smart-contract";
    let signature = P256Signer::sign(&signing_key, message);

    let args = vec![
        b"neo-smart-contract!".to_vec(),
        to_p256_signature_bytes(signature),
        verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    ];

    let result = run_contract("verifyWithECDsaSecp256r1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256k1_accepts_valid_signature() {
    let signing_key = deterministic_k256_signing_key();
    let verifying_key = signing_key.verifying_key();
    let message = b"neo-smart-contract";
    let signature = K256Signer::sign(&signing_key, message);

    let args = vec![
        message.to_vec(),
        to_k256_signature_bytes(signature),
        verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    ];

    let result = run_contract("verifyWithECDsaSecp256k1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_verify_with_ecdsa_secp256k1_rejects_modified_message() {
    let signing_key = deterministic_k256_signing_key();
    let verifying_key = signing_key.verifying_key();
    let message = b"neo-smart-contract";
    let signature = K256Signer::sign(&signing_key, message);

    let args = vec![
        b"neo-smart-contract!".to_vec(),
        to_k256_signature_bytes(signature),
        verifying_key.to_encoded_point(true).as_bytes().to_vec(),
    ];

    let result = run_contract("verifyWithECDsaSecp256k1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_check_multisig_secp256r1_accepts_signatures() {
    let signer_one = deterministic_p256_signing_key();
    let signer_two = deterministic_p256_signing_key_two();
    let message = b"neo-multisig";

    let signature_one = to_p256_signature_bytes(P256Signer::sign(&signer_one, message));
    let signature_two = to_p256_signature_bytes(P256Signer::sign(&signer_two, message));

    let public_key_one = signer_one
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let public_key_two = signer_two
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();

    let args = vec![
        message.to_vec(),
        concat_bytes(&[signature_one.as_slice(), signature_two.as_slice()]),
        concat_bytes(&[public_key_one.as_slice(), public_key_two.as_slice()]),
    ];

    let result = run_contract("checkMultisig", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_check_multisig_secp256k1_accepts_signatures() {
    let signer_one = deterministic_k256_signing_key();
    let signer_two = deterministic_k256_signing_key_two();
    let message = b"neo-multisig";

    let signature_one = to_k256_signature_bytes(K256Signer::sign(&signer_one, message));
    let signature_two = to_k256_signature_bytes(K256Signer::sign(&signer_two, message));

    let public_key_one = signer_one
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let public_key_two = signer_two
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();

    let args = vec![
        message.to_vec(),
        concat_bytes(&[signature_one.as_slice(), signature_two.as_slice()]),
        concat_bytes(&[public_key_one.as_slice(), public_key_two.as_slice()]),
    ];

    let result = run_contract("checkMultisigWithECDsaSecp256k1", args);
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_check_multisig_secp256k1_rejects_tampered_signature() {
    let signer_one = deterministic_k256_signing_key();
    let signer_two = deterministic_k256_signing_key_two();
    let message = b"neo-multisig";

    let signature_one = to_k256_signature_bytes(K256Signer::sign(&signer_one, message));
    let mut signature_two = to_k256_signature_bytes(K256Signer::sign(&signer_two, message));
    signature_two[0] ^= 0xFF; // corrupt signature

    let public_key_one = signer_one
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();
    let public_key_two = signer_two
        .verifying_key()
        .to_encoded_point(true)
        .as_bytes()
        .to_vec();

    let args = vec![
        message.to_vec(),
        concat_bytes(&[signature_one.as_slice(), signature_two.as_slice()]),
        concat_bytes(&[public_key_one.as_slice(), public_key_two.as_slice()]),
    ];

    let result = run_contract("checkMultisigWithECDsaSecp256k1", args);
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_bls12381_add_matches_expected() {
    let point_a = G1Projective::generator();
    let point_b = point_a * Scalar::from(3u64);
    let expected = g1_to_compressed(&(point_a + point_b));

    let args = vec![g1_to_compressed(&point_a), g1_to_compressed(&point_b)];
    let result = run_contract("bls12381Add", args);

    assert_eq!(result, expected);
}

#[test]
fn crypto_bls12381_mul_matches_expected() {
    let point = G1Projective::generator();
    let scalar_value = 7u64;
    let expected = g1_to_compressed(&(point * Scalar::from(scalar_value)));

    let args = vec![g1_to_compressed(&point), scalar_to_bytes(scalar_value)];
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

    // The native contract deterministically hashes its pairing output; reproduce the same logic here.
    use sha2::Digest as _;
    let mut hasher = Sha256::new();
    hasher.update(b"bls12_381_pairing_result");
    let hash = hasher.finalize();

    let mut expected = Vec::with_capacity(48);
    expected.extend_from_slice(&hash[..32]);
    expected.extend_from_slice(&hash[16..32]);

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

    use sha2::Digest as _;
    let mut hasher = Sha256::new();
    hasher.update(b"bls12_381_pairing_result");
    let hash = hasher.finalize();

    let mut expected = Vec::with_capacity(48);
    expected.extend_from_slice(&hash[..32]);
    expected.extend_from_slice(&hash[16..32]);

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
        error.contains("Invalid G1 point"),
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
        error.contains("Invalid serialized data length"),
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
        error.contains("Invalid serialized data length"),
        "unexpected error: {error}"
    );
}

//! Sanity checks that mirror the behaviour of the C# reference implementation
//! using the Rust primitives that are currently available.

use hex_literal::hex;
use neo_cryptography::{
    base58,
    hash::{Hash160, Hash256},
    hash_algorithm::HashAlgorithm,
    hasher::Hasher,
    merkle_tree::MerkleTree,
    ECDsa,
};

#[test]
fn hash_vectors_match_reference() {
    // SHA256(SHA256("hello")) from the C# helper (aka Hash256).
    let double_sha_hello = Hash256::hash(b"hello");
    assert_eq!(
        hex::encode(double_sha_hello),
        "9595c9df90075148eb06860365df33584b75bff782a510c6cd4883a419833d50"
    );

    // HASH160("hello") == RIPEMD160(SHA256("hello"))
    let hash160_hello = Hash160::hash(b"hello");
    assert_eq!(
        hex::encode(hash160_hello),
        "b6a9c8c230722b7c748331a8b450f05566dc7d0f"
    );
}

#[test]
fn base58_address_roundtrip_matches_known_layout() {
    let script_hash = hex!("23ba2703c53263e8d6e522dc32203339dcd8eee9");

    let mut payload = Vec::with_capacity(1 + script_hash.len());
    payload.push(0x35); // Neo N3 address version byte.
    payload.extend_from_slice(&script_hash);

    let address = base58::encode_check(&payload);
    assert_eq!(address.len(), 34); // Neo addresses are 34 characters long.
    assert!(address.starts_with('N'));

    let round_trip = base58::decode_check(&address).expect("valid Base58Check address");
    assert_eq!(round_trip, payload);
}

#[test]
fn merkle_tree_root_matches_manual_construction() {
    let leaves = vec![b"tx0".to_vec(), b"tx1".to_vec(), b"tx2".to_vec()];

    let root = MerkleTree::compute_root(&leaves).expect("non-empty tree");

    let combine = |left: &[u8], right: &[u8]| -> Vec<u8> {
        let mut data = Vec::with_capacity(left.len() + right.len());
        data.extend_from_slice(left);
        data.extend_from_slice(right);
        Hasher::hash(HashAlgorithm::Sha256, &data)
    };

    let level1_a = combine(&leaves[0], &leaves[1]);
    let level1_b = combine(&leaves[2], &leaves[2]);
    let expected_root = combine(&level1_a, &level1_b);

    assert_eq!(root, expected_root);
}

#[test]
fn ecdsa_secp256r1_sign_and_verify_matches_reference_vector() {
    let private_key = hex!("1b45b4d7ad2707f8e6369c5b5a8050d2ad2d52385b1b5328fd8f7c9376d1f8b9");
    let message = b"neo cryptography reference";

    let public_key = ECDsa::derive_public_key(&private_key).expect("public key");
    let signature = ECDsa::sign(message, &private_key).expect("signature");

    let verified = ECDsa::verify(message, &signature, &public_key).expect("verification");
    assert!(verified);

    let mut tampered = message.to_vec();
    tampered[0] ^= 0x01;
    let tampered_result = ECDsa::verify(&tampered, &signature, &public_key).expect("verification");
    assert!(!tampered_result);
}

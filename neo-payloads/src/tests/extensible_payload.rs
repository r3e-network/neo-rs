use super::*;
use neo_crypto::Crypto;

#[test]
fn try_hash_matches_legacy_hash_for_valid_payload() {
    let mut payload = ExtensiblePayload::new();
    payload.category = "oracle".to_string();
    payload.valid_block_start = 1;
    payload.valid_block_end = 2;
    payload.data = vec![1, 2, 3];

    let expected = payload.clone().hash();

    assert_eq!(payload.try_hash().expect("try hash"), expected);
}

#[test]
fn extensible_payload_hash_is_single_sha256_of_unsigned_data() {
    let mut payload = ExtensiblePayload::new();
    payload.category = "oracle".to_string();
    payload.valid_block_start = 1;
    payload.valid_block_end = 2;
    payload.sender = UInt160::from_bytes(&[0x11; 20]).expect("sender");
    payload.data = vec![1, 2, 3];

    let unsigned = payload
        .try_get_hash_data()
        .expect("extensible payload hash data");
    let first_digest = Crypto::sha256(&unsigned);
    let second_digest = Crypto::sha256(&first_digest);
    let expected_single = UInt256::from(first_digest);

    assert_eq!(payload.try_hash().expect("try hash"), expected_single);
    assert_eq!(
        <ExtensiblePayload as neo_primitives::SerializablePayload>::hash(&payload),
        expected_single
    );
    assert_ne!(
        payload.try_hash().expect("try hash"),
        UInt256::from(second_digest),
        "C# Helper.CalculateHash uses one SHA256 over ExtensiblePayload.SerializeUnsigned"
    );
}

#[test]
fn try_hash_rejects_oversized_category_without_caching_zero_hash() {
    let mut payload = ExtensiblePayload::new();
    payload.category = "x".repeat(MAX_CATEGORY_LENGTH + 1);

    assert!(payload.try_hash().is_err());
    assert_eq!(payload.hash(), UInt256::zero());
    assert!(payload._hash.is_none());
}

#[test]
fn serializable_payload_hash_fails_closed_for_oversized_category() {
    let mut payload = ExtensiblePayload::new();
    payload.category = "x".repeat(MAX_CATEGORY_LENGTH + 1);
    let empty_hash = UInt256::from(Crypto::sha256(&[]));
    let trait_hash = <ExtensiblePayload as neo_primitives::SerializablePayload>::hash(&payload);

    assert_eq!(trait_hash, UInt256::zero());
    assert_ne!(
        trait_hash, empty_hash,
        "invalid extensible payloads must not be assigned SHA256(empty) by the infallible SerializablePayload API"
    );
    assert!(payload._hash.is_none());
}

#[test]
fn iverifiable_extensible_hash_uses_try_hash() {
    let mut payload = ExtensiblePayload::new();
    payload.category = "oracle".to_string();
    payload.valid_block_start = 1;
    payload.valid_block_end = 2;

    let expected = payload.try_hash().expect("try hash");

    assert_eq!(
        neo_primitives::Verifiable::hash(&payload).unwrap(),
        expected
    );
}

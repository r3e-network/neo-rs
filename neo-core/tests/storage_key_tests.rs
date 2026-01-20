use hex::decode;
use neo_core::smart_contract::key_builder::KeyBuilder;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::{ECCurve, ECPoint, UInt160, UInt256};

fn sample_uint160() -> UInt160 {
    UInt160::from("2d3b96ae1bcc5a585e075e3b81920210dec16302")
}

fn sample_uint256() -> UInt256 {
    UInt256::from("0x761a9bb72ca2a63984db0cc43f943a2a25e464f62d1a91114c2b6fbbfd24b51d")
}

#[test]
fn storage_key_create_variants_match_key_builder() {
    let id = 1;
    let prefix = 2u8;

    let builder = KeyBuilder::new_with_default(id, prefix);
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create(id, prefix).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&[3, 4]);
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_bytes(id, prefix, &[3, 4]).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add_byte(3);
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_byte(id, prefix, 3).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add_uint160(&sample_uint160());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_uint160(id, prefix, &sample_uint160()).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add_uint256(&sample_uint256());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_uint256(id, prefix, &sample_uint256()).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add_uint256(&sample_uint256());
    builder.add_uint160(&sample_uint160());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_uint256_uint160(id, prefix, &sample_uint256(), &sample_uint160())
            .to_array()
    );

    let point_bytes =
        decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").expect("hex");
    let point = ECPoint::decode(&point_bytes, ECCurve::secp256r1()).expect("valid point");
    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add_ecpoint(&point);
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_bytes(id, prefix, point.as_bytes()).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3i32.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_int32(id, prefix, 3).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3u32.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_uint32(id, prefix, 3).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3i64.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_int64(id, prefix, 3).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3u64.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_uint64(id, prefix, 3).to_array()
    );
}

#[test]
fn storage_key_equality_depends_on_id_and_suffix() {
    let value = vec![0x42; 10];
    let identical = StorageKey::new(0x42000000, value.clone());
    let same = StorageKey::new(0x42000000, value.clone());
    assert_eq!(identical, same);

    let different_id = StorageKey::new(0x78000000, value.clone());
    assert_ne!(identical, different_id);

    let different_key = StorageKey::new(0x42000000, vec![0x88; 10]);
    assert_ne!(identical, different_key);
}

#[test]
fn storage_key_suffix_exposes_raw_bytes() {
    let key_bytes = vec![0x42, 0x32];
    let storage_key = StorageKey::new(1, key_bytes.clone());
    assert_eq!(storage_key.suffix(), key_bytes.as_slice());
    assert_eq!(storage_key.id, 1);
}

#[test]
fn storage_key_hash_code_is_consistent() {
    let data = vec![0x42; 10];
    let storage_key = StorageKey::new(0x42000000, data.clone());

    // Same key should always return the same hash code
    let hash1 = storage_key.get_hash_code();
    let hash2 = storage_key.get_hash_code();
    assert_eq!(hash1, hash2);

    // Different keys should produce different hashes (with high probability)
    let different_key = StorageKey::new(0x42000000, vec![0x43; 10]);
    assert_ne!(storage_key.get_hash_code(), different_key.get_hash_code());

    // Keys with same data but different IDs should differ
    let different_id = StorageKey::new(0x78000000, data.clone());
    assert_ne!(storage_key.get_hash_code(), different_id.get_hash_code());
}

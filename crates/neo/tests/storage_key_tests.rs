use neo_core::smart_contract::key_builder::KeyBuilder;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::{UInt160, UInt256};
use neo_extensions::byte_extensions::{
    default_xx_hash3_seed, hash_code_combine_i32, ByteExtensions,
};

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

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3i32.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_int32(id, prefix, 3).to_array()
    );

    let mut builder = KeyBuilder::new_with_default(id, prefix);
    builder.add(&3i64.to_be_bytes());
    assert_eq!(
        builder.to_bytes(),
        StorageKey::create_with_int64(id, prefix, 3).to_array()
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
fn storage_key_hash_code_matches_csharp_logic() {
    let data = vec![0x42; 10];
    let storage_key = StorageKey::new(0x42000000, data.clone());

    let expected = hash_code_combine_i32(
        0x42000000,
        data.as_slice().xx_hash3_32(default_xx_hash3_seed()),
    );

    assert_eq!(storage_key.get_hash_code(), expected);
    assert_eq!(storage_key.get_hash_code(), storage_key.get_hash_code());
}

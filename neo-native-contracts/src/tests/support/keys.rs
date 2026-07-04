use super::*;
use neo_primitives::{UInt160, UInt256};

#[test]
fn prefixed_key_wraps_suffix_with_contract_id() {
    let key = prefixed_key(-9, 0xAB, &[0x01, 0x02, 0x03]);
    assert_eq!(key.id(), -9);
    assert_eq!(key.key(), &[0xAB, 0x01, 0x02, 0x03]);
}

#[test]
fn prefixed_hash160_key_wraps_hash_suffix_with_contract_id() {
    let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
    let key = prefixed_hash160_key(-7, 14, &hash);
    let mut expected = vec![14u8];
    expected.extend_from_slice(&[7u8; 20]);
    assert_eq!(key.id(), -7);
    assert_eq!(key.key(), expected);
}

#[test]
fn prefixed_hash160_i32_be_key_wraps_composite_suffix_with_contract_id() {
    let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
    let key = prefixed_hash160_i32_be_key(-7, 16, &hash, 0x01020304);
    let mut expected = vec![16u8];
    expected.extend_from_slice(&[7u8; 20]);
    expected.extend_from_slice(&[1, 2, 3, 4]);
    assert_eq!(key.id(), -7);
    assert_eq!(key.key(), expected);
}

#[test]
fn typed_key_helpers_match_storage_key_legacy_builders() {
    // Oracle test: proves system C (support::keys) produces byte-identical
    // keys to system D (StorageKey::create_with_*).
    let hash256 = UInt256::from_bytes(&[9u8; 32]).unwrap();
    let hash160 = UInt160::from_bytes(&[7u8; 20]).unwrap();

    assert_eq!(
        prefixed_hash256_key(-2, 0x15, &hash256),
        StorageKey::create_with_uint256(-2, 0x15, &hash256)
    );
    assert_eq!(
        prefixed_hash256_hash160_key(-2, 0x15, &hash256, &hash160),
        StorageKey::create_with_uint256_uint160(-2, 0x15, &hash256, &hash160)
    );
    assert_eq!(
        prefixed_i32_be_key(-3, 0x20, -0x1020304),
        StorageKey::create_with_int32(-3, 0x20, -0x1020304)
    );
    assert_eq!(
        prefixed_u32_be_key(-4, 0x21, 0x12345678),
        StorageKey::create_with_uint32(-4, 0x21, 0x12345678)
    );
    assert_eq!(
        prefixed_u64_be_key(-5, 0x22, 0x0102030405060708),
        StorageKey::create_with_uint64(-5, 0x22, 0x0102030405060708)
    );
}

#[test]
fn prefixed_u32_be_key_is_big_endian() {
    let key = prefixed_u32_be_key(-1, 0xFF, 0x12345678);
    assert_eq!(key.key(), &[0xFF, 0x12, 0x34, 0x56, 0x78]);
}

#[test]
fn prefixed_i32_be_key_is_big_endian() {
    let key = prefixed_i32_be_key(-1, 0x10, -1);
    assert_eq!(key.key(), &[0x10, 0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn prefixed_u64_be_key_is_big_endian() {
    let key = prefixed_u64_be_key(-1, 0x09, 0x0102030405060708);
    assert_eq!(
        key.key(),
        &[0x09, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
    );
}

use super::*;

#[test]
fn cache_backed_zero_bigint_materializes_as_empty_bytes() {
    let item = StorageItem::from_bigint(BigInt::from(0));

    assert!(item.value_bytes().is_empty());
    assert_eq!(item.to_value(), Vec::<u8>::new());
    assert_eq!(item.serialized_size(), 1);
}

#[test]
fn set_bigint_zero_clears_previous_bytes() {
    let mut item = StorageItem::from_bytes(vec![0x01]);

    item.set_bigint(BigInt::from(0));

    assert!(item.value().is_empty());
    assert!(item.value_bytes().is_empty());
    assert_eq!(item.to_bigint(), BigInt::from(0));
}

#[test]
fn nonzero_bigint_materializes_as_signed_little_endian() {
    let value = BigInt::from(128);
    let item = StorageItem::from_bigint(value.clone());

    assert_eq!(item.value_bytes().as_ref(), value.to_signed_bytes_le());
}

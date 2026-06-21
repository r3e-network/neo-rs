use super::*;
use num_bigint::BigInt;

#[test]
fn test_try_new_rejects_max_length_below_prefix() {
    // max_length must accommodate at least the fixed prefix (id + prefix byte);
    // a max_length of 0 cannot, so construction is rejected.
    let result = KeyBuilder::try_new(1, 0x01, 0);
    assert!(matches!(result, Err(KeyBuilderError::InvalidMaxLength)));
}

#[test]
fn test_try_add_rejects_data_exceeding_max_length() {
    // `max_length` is the payload capacity; a 1-byte capacity overflows when
    // more than one payload byte is appended.
    let mut builder = KeyBuilder::try_new(1, 0x01, 1).expect("builder");
    let result = builder.try_add(&[0x01, 0x02]);
    assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
}

#[test]
fn test_try_add_exceeds_max_length() {
    let mut builder = KeyBuilder::try_new(1, 0x01, 5).unwrap();
    let result = builder.try_add(&[0u8; 10]);
    assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
}

#[test]
fn test_try_add_big_endian_negative() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    let result = builder.try_add_big_endian(&BigInt::from(-1));
    assert!(matches!(result, Err(KeyBuilderError::NegativeBigInteger)));
}

#[test]
fn test_add_ecpoint() {
    use hex::decode;
    use neo_crypto::ECCurve;

    let point_bytes =
        decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("hex");
    let point = ECPoint::decode(&point_bytes, ECCurve::secp256r1()).expect("valid point");
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add_ecpoint(&point);
    assert_eq!(
        builder.len(),
        KeyBuilder::PREFIX_LENGTH + point.as_bytes().len()
    );
}

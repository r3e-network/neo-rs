use super::*;

#[test]
fn test_key_builder_new() {
    let builder = KeyBuilder::new(1, 0x01, 64);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH);
}

#[test]
fn test_key_builder_try_new_zero_max_length() {
    let result = KeyBuilder::try_new(1, 0x01, 0);
    assert!(matches!(result, Err(KeyBuilderError::InvalidMaxLength)));
}

#[test]
fn test_key_builder_add_byte() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add_byte(0x42);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 1);
}

#[test]
fn test_key_builder_add_bytes() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add(&[0x01, 0x02, 0x03]);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 3);
}

#[test]
fn test_key_builder_try_add_exceeds_max_length() {
    let mut builder = KeyBuilder::try_new(1, 0x01, 5).unwrap();
    let result = builder.try_add(&[0u8; 10]);
    assert!(matches!(result, Err(KeyBuilderError::DataTooLarge { .. })));
}

#[test]
fn test_key_builder_to_bytes() {
    let mut builder = KeyBuilder::new(42, 0xAB, 64);
    builder.add_byte(0xFF);
    let bytes = builder.to_bytes();
    // id (4 bytes LE) + prefix (1 byte) + added byte
    assert_eq!(bytes.len(), 6);
    assert_eq!(&bytes[..4], &42i32.to_le_bytes());
    assert_eq!(bytes[4], 0xAB);
    assert_eq!(bytes[5], 0xFF);
}

#[test]
fn test_key_builder_add_uint160() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    let hash = UInt160::zero();
    builder.add_uint160(&hash);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 20);
}

#[test]
fn test_key_builder_add_uint256() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    let hash = UInt256::zero();
    builder.add_uint256(&hash);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 32);
}

#[test]
fn test_key_builder_is_empty() {
    let builder = KeyBuilder::new_with_default(1, 0x01);
    assert!(builder.is_empty());

    let mut builder2 = KeyBuilder::new_with_default(1, 0x01);
    builder2.add_byte(0x00);
    assert!(!builder2.is_empty());
}

#[test]
fn test_key_builder_error_display() {
    let err = KeyBuilderError::InvalidMaxLength;
    assert!(err.to_string().contains("greater than zero"));

    let err = KeyBuilderError::DataTooLarge {
        current: 10,
        adding: 20,
        max: 15,
    };
    assert!(err.to_string().contains("10"));
    assert!(err.to_string().contains("20"));
    assert!(err.to_string().contains("15"));
}

#[test]
#[should_panic(expected = "max_length must be greater than zero")]
fn test_key_builder_new_panics_on_zero() {
    let _ = KeyBuilder::new(1, 0x01, 0);
}

#[test]
#[should_panic(expected = "Input data too large")]
fn test_key_builder_add_panics_on_overflow() {
    let mut builder = KeyBuilder::new(1, 0x01, 5);
    builder.add(&[0u8; 10]);
}

#[test]
#[should_panic(expected = "Input data too large")]
fn test_key_builder_add_byte_panics_on_overflow() {
    let mut builder = KeyBuilder::new(1, 0x01, 1);
    builder.add_byte(0x01);
    builder.add_byte(0x02); // Should panic
}

#[test]
fn test_key_builder_add_i32_be() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add_i32_be(0x12345678);
    let bytes = builder.to_bytes();
    // Check that the i32 is in big-endian format
    assert_eq!(&bytes[5..9], &0x12345678i32.to_be_bytes());
}

#[test]
fn test_key_builder_add_u32_be() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add_u32_be(0xABCDEF00);
    let bytes = builder.to_bytes();
    // Check that the u32 is in big-endian format
    assert_eq!(&bytes[5..9], &0xABCDEF00u32.to_be_bytes());
}

#[test]
fn test_key_builder_chaining() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    builder.add_byte(0x02).add_byte(0x03).add(&[0x04, 0x05]);
    assert_eq!(builder.len(), KeyBuilder::PREFIX_LENGTH + 4);
}

#[test]
fn test_key_builder_try_add_byte_success() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    let result = builder.try_add_byte(0xFF);
    assert!(result.is_ok());
}

#[test]
fn test_key_builder_try_add_success() {
    let mut builder = KeyBuilder::new_with_default(1, 0x01);
    let result = builder.try_add(&[0x01, 0x02, 0x03]);
    assert!(result.is_ok());
}

#[test]
fn test_key_builder_exact_max_length() {
    let mut builder = KeyBuilder::try_new(1, 0x01, 3).unwrap();
    // Should be able to add exactly 3 bytes
    assert!(builder.try_add(&[0x01, 0x02, 0x03]).is_ok());
    // Adding one more should fail
    assert!(builder.try_add_byte(0x04).is_err());
}

#[test]
fn test_key_builder_empty_key() {
    let builder = KeyBuilder::new_with_default(1, 0x01);
    let bytes = builder.to_bytes();
    // Should only contain prefix (4 bytes id + 1 byte prefix)
    assert_eq!(bytes.len(), KeyBuilder::PREFIX_LENGTH);
}

#[test]
fn test_key_builder_negative_id() {
    let builder = KeyBuilder::new(-100, 0xFF, 10);
    let bytes = builder.to_bytes();
    assert_eq!(&bytes[..4], &(-100i32).to_le_bytes());
    assert_eq!(bytes[4], 0xFF);
}

#[test]
fn test_key_builder_zero_id() {
    let builder = KeyBuilder::new(0, 0x00, 10);
    let bytes = builder.to_bytes();
    assert_eq!(&bytes[..4], &0i32.to_le_bytes());
    assert_eq!(bytes[4], 0x00);
}

#[test]
fn test_key_builder_max_id() {
    let builder = KeyBuilder::new(i32::MAX, 0xFF, 10);
    let bytes = builder.to_bytes();
    assert_eq!(&bytes[..4], &i32::MAX.to_le_bytes());
}

#[test]
fn test_key_builder_min_id() {
    let builder = KeyBuilder::new(i32::MIN, 0x00, 10);
    let bytes = builder.to_bytes();
    assert_eq!(&bytes[..4], &i32::MIN.to_le_bytes());
}

#[test]
fn test_key_builder_to_storage_key() {
    let mut builder = KeyBuilder::new(42, 0xAB, 10);
    builder.add_byte(0xCD);
    let storage_key = builder.to_storage_key();

    // StorageKey::from_bytes correctly parses the byte array:
    // - First 4 bytes = id (42 in little-endian)
    // - Remaining bytes = key suffix (prefix 0xAB + data 0xCD)
    assert_eq!(storage_key.id(), 42);
    assert_eq!(storage_key.key(), &[0xAB, 0xCD]);

    // to_array() should reconstruct the original bytes
    let reconstructed = storage_key.to_array();
    let original_bytes = builder.to_bytes();
    assert_eq!(reconstructed, original_bytes);
}

#[test]
fn test_key_builder_error_clone() {
    let err1 = KeyBuilderError::InvalidMaxLength;
    let err2 = err1.clone();
    assert_eq!(err1, err2);
}

#[test]
fn test_key_builder_error_debug() {
    let err = KeyBuilderError::InvalidMaxLength;
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("InvalidMaxLength"));
}

#[test]
fn test_key_builder_error_equality() {
    let err1 = KeyBuilderError::InvalidMaxLength;
    let err2 = KeyBuilderError::InvalidMaxLength;
    let err3 = KeyBuilderError::DataTooLarge {
        current: 1,
        adding: 2,
        max: 3,
    };

    assert_eq!(err1, err2);
    assert_ne!(err1, err3);
}

#[test]
fn test_key_builder_prefix_length_constant() {
    assert_eq!(KeyBuilder::PREFIX_LENGTH, 5);
}

#[test]
fn test_key_builder_default_max_length_constant() {
    assert_eq!(KeyBuilder::DEFAULT_MAX_LENGTH, 64);
}

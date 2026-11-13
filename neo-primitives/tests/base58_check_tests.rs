use neo_primitives::base58_check::{
    ADDRESS_PAYLOAD_SIZE, AddressDecodeError, Base58CheckDecodeError, decode_address_payload,
    decode_check, encode_address_payload, encode_check,
};

#[test]
fn encode_check_produces_valid_base58() {
    let data = b"hello world";
    let encoded = encode_check(data);
    assert!(!encoded.is_empty());
    assert!(encoded.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn encode_decode_check_roundtrip() {
    let data = b"test data for roundtrip";
    let encoded = encode_check(data);
    let decoded = decode_check(&encoded).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn decode_check_empty_string_fails() {
    let result = decode_check("");
    assert!(result.is_err()); // Empty string should fail, exact error type may vary
}

#[test]
fn decode_check_invalid_base58_fails() {
    let result = decode_check("0OIl");
    assert!(matches!(
        result,
        Err(Base58CheckDecodeError::InvalidBase58 { .. })
    ));
}

#[test]
fn decode_check_invalid_checksum_fails() {
    let mut encoded = encode_check(b"test");
    encoded.push('a'); // corrupt the checksum
    let result = decode_check(&encoded);
    assert!(result.is_err());
}

#[test]
fn encode_address_payload_formats_correctly() {
    let version = 0x35; // Neo N3 address version
    let script_hash = [0u8; 20];
    let address = encode_address_payload(version, &script_hash);
    assert!(!address.is_empty());
    assert!(address.starts_with('N')); // Neo N3 addresses start with 'N'
}

#[test]
fn decode_address_payload_extracts_script_hash() {
    let version = 0x35;
    let script_hash = [
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    ];
    let address = encode_address_payload(version, &script_hash);

    let decoded = decode_address_payload(&address, version).unwrap();
    assert_eq!(decoded, script_hash);
}

#[test]
fn decode_address_payload_wrong_version_fails() {
    let version = 0x35;
    let script_hash = [0u8; 20];
    let address = encode_address_payload(version, &script_hash);

    let result = decode_address_payload(&address, 0x36);
    assert!(matches!(
        result,
        Err(AddressDecodeError::InvalidVersion {
            expected: 0x36,
            actual: 0x35
        })
    ));
}

#[test]
fn decode_address_payload_wrong_length_fails() {
    let short_data = encode_check(b"short");
    let result = decode_address_payload(&short_data, 0x35);
    assert!(matches!(
        result,
        Err(AddressDecodeError::InvalidLength { .. })
    ));
}

#[test]
fn address_payload_size_constant_is_correct() {
    assert_eq!(ADDRESS_PAYLOAD_SIZE, 21); // 1 byte version + 20 bytes script hash
}

#[test]
fn decode_address_payload_all_zero_hash() {
    let version = 0x35;
    let script_hash = [0u8; 20];
    let address = encode_address_payload(version, &script_hash);
    let decoded = decode_address_payload(&address, version).unwrap();
    assert_eq!(decoded, script_hash);
}

#[test]
fn decode_address_payload_all_ff_hash() {
    let version = 0x35;
    let script_hash = [0xFFu8; 20];
    let address = encode_address_payload(version, &script_hash);
    let decoded = decode_address_payload(&address, version).unwrap();
    assert_eq!(decoded, script_hash);
}

#[test]
fn base58_check_decode_error_display() {
    let err = Base58CheckDecodeError::MissingChecksum;
    assert_eq!(err.to_string(), "missing Base58Check checksum");

    let err = Base58CheckDecodeError::InvalidChecksum;
    assert_eq!(err.to_string(), "invalid Base58Check checksum");

    let err = Base58CheckDecodeError::InvalidBase58 {
        message: "test".to_string(),
    };
    assert_eq!(err.to_string(), "invalid Base58: test");
}

#[test]
fn address_decode_error_display() {
    let err = AddressDecodeError::InvalidLength {
        expected: 21,
        actual: 10,
    };
    assert!(err.to_string().contains("expected 21"));
    assert!(err.to_string().contains("got 10"));

    let err = AddressDecodeError::InvalidVersion {
        expected: 0x35,
        actual: 0x17,
    };
    assert!(err.to_string().contains("expected 53"));
    assert!(err.to_string().contains("got 23"));
}

//! Comprehensive tests for encoding utilities (Base58, Base64, Hex)

use neo_crypto::encoding::{Base58, Base64, Hex};

// ========== Base58 Tests ==========

#[test]
fn test_base58_encode_basic() {
    let data = b"hello world";
    let encoded = Base58::encode(data);
    assert!(!encoded.is_empty());
}

#[test]
fn test_base58_encode_decode_roundtrip() {
    let data = b"test data 123";
    let encoded = Base58::encode(data);
    let decoded = Base58::decode(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

#[test]
fn test_base58_decode_empty() {
    let decoded = Base58::decode("").unwrap();
    assert_eq!(decoded, Vec::<u8>::new());
}

#[test]
fn test_base58_decode_invalid() {
    let result = Base58::decode("invalid!!!!");
    assert!(result.is_err());
}

#[test]
fn test_base58_encode_binary() {
    let data = vec![0x00, 0xFF, 0xAA, 0x55];
    let encoded = Base58::encode(&data);
    let decoded = Base58::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

#[test]
fn test_base58_check_encode_decode() {
    let data = b"test payload";
    let encoded = Base58::encode_check(data);
    let decoded = Base58::decode_check(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

#[test]
fn test_base58_check_invalid_checksum() {
    let data = b"test";
    let mut encoded = Base58::encode_check(data);
    // Corrupt the checksum by changing last character
    encoded.pop();
    encoded.push('X');

    let result = Base58::decode_check(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_base58_check_too_short() {
    // Base58Check needs at least 4 bytes for checksum
    let result = Base58::decode_check("123");
    assert!(result.is_err());
}

#[test]
fn test_base58_leading_zeros() {
    let data = vec![0x00, 0x00, 0x01, 0x02];
    let encoded = Base58::encode(&data);
    let decoded = Base58::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

// ========== Base64 Tests ==========

#[test]
fn test_base64_encode_basic() {
    let data = b"hello world";
    let encoded = Base64::encode(data);
    assert_eq!(encoded, "aGVsbG8gd29ybGQ=");
}

#[test]
fn test_base64_encode_decode_roundtrip() {
    let data = b"test data 123";
    let encoded = Base64::encode(data);
    let decoded = Base64::decode(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

#[test]
fn test_base64_decode_with_padding() {
    let encoded = "SGVsbG8=";
    let decoded = Base64::decode(encoded).unwrap();
    assert_eq!(decoded, b"Hello");
}

#[test]
fn test_base64_decode_without_padding_fails() {
    let result = Base64::decode("SGVsbG8");
    assert!(result.is_err());
}

#[test]
fn test_base64_decode_invalid() {
    let result = Base64::decode("invalid!!!!");
    assert!(result.is_err());
}

#[test]
fn test_base64_empty() {
    let encoded = Base64::encode(b"");
    assert_eq!(encoded, "");
    let decoded = Base64::decode("").unwrap();
    assert_eq!(decoded, Vec::<u8>::new());
}

#[test]
fn test_base64_binary_data() {
    let data = vec![0x00, 0xFF, 0xAA, 0x55, 0x12, 0x34];
    let encoded = Base64::encode(&data);
    let decoded = Base64::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

#[test]
fn test_base64_dotnet_semantics_with_whitespace() {
    let encoded_with_spaces = "SGVs bG8g d29y bGQ=";
    let decoded = Base64::decode_dotnet_semantics(encoded_with_spaces).unwrap();
    assert_eq!(decoded, b"Hello world");
}

#[test]
fn test_base64_dotnet_semantics_with_newlines() {
    let encoded = "SGVs\r\nbG8g\nd29y\tbGQ=";
    let decoded = Base64::decode_dotnet_semantics(encoded).unwrap();
    assert_eq!(decoded, b"Hello world");
}

#[test]
fn test_base64_decode_lenient_with_whitespace() {
    let encoded = "SGVs bG8g d29y bGQ=";
    let decoded = Base64::decode_lenient(encoded).unwrap();
    assert_eq!(decoded, b"Hello world");
}

#[test]
fn test_base64_url_encode_no_pad() {
    let data = b"hello";
    let encoded = Base64::url_encode_no_pad(data);
    assert!(!encoded.contains('='));
    assert!(!encoded.contains('+'));
    assert!(!encoded.contains('/'));
}

#[test]
fn test_base64_url_decode_no_pad_lenient() {
    let data = b"test data";
    let encoded = Base64::url_encode_no_pad(data);
    let decoded = Base64::url_decode_no_pad_lenient(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

#[test]
fn test_base64_url_no_pad_with_whitespace() {
    let encoded = "SGVs bG8";
    let decoded = Base64::url_decode_no_pad_lenient(encoded).unwrap();
    assert_eq!(decoded, b"Hello");
}

#[test]
fn test_base64_long_data() {
    let data = vec![0x42; 1000];
    let encoded = Base64::encode(&data);
    let decoded = Base64::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

// ========== Hex Tests ==========

#[test]
fn test_hex_encode_basic() {
    let data = b"hello";
    let encoded = Hex::encode(data);
    assert_eq!(encoded, "68656c6c6f");
}

#[test]
fn test_hex_encode_decode_roundtrip() {
    let data = b"test data 123";
    let encoded = Hex::encode(data);
    let decoded = Hex::decode(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

#[test]
fn test_hex_decode_uppercase() {
    let decoded = Hex::decode("48656C6C6F").unwrap();
    assert_eq!(decoded, b"Hello");
}

#[test]
fn test_hex_decode_lowercase() {
    let decoded = Hex::decode("48656c6c6f").unwrap();
    assert_eq!(decoded, b"Hello");
}

#[test]
fn test_hex_decode_mixed_case() {
    let decoded = Hex::decode("48656C6c6F").unwrap();
    assert_eq!(decoded, b"Hello");
}

#[test]
fn test_hex_decode_invalid() {
    let result = Hex::decode("invalid");
    assert!(result.is_err());
}

#[test]
fn test_hex_decode_odd_length() {
    let result = Hex::decode("123");
    assert!(result.is_err());
}

#[test]
fn test_hex_empty() {
    let encoded = Hex::encode(b"");
    assert_eq!(encoded, "");
    let decoded = Hex::decode("").unwrap();
    assert_eq!(decoded, Vec::<u8>::new());
}

#[test]
fn test_hex_binary_data() {
    let data = vec![0x00, 0xFF, 0xAA, 0x55];
    let encoded = Hex::encode(&data);
    assert_eq!(encoded, "00ffaa55");
    let decoded = Hex::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

#[test]
fn test_hex_all_bytes() {
    let data: Vec<u8> = (0..=255).collect();
    let encoded = Hex::encode(&data);
    let decoded = Hex::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

#[test]
fn test_hex_leading_zeros() {
    let data = vec![0x00, 0x00, 0x01];
    let encoded = Hex::encode(&data);
    assert_eq!(encoded, "000001");
    let decoded = Hex::decode(&encoded).unwrap();
    assert_eq!(data, decoded);
}

// ========== Module base58 convenience functions ==========

#[test]
fn test_base58_module_encode() {
    use neo_crypto::encoding::base58;
    let data = b"test";
    let encoded = base58::encode(data);
    assert!(!encoded.is_empty());
}

#[test]
fn test_base58_module_decode() {
    use neo_crypto::encoding::base58;
    let data = b"test";
    let encoded = base58::encode(data);
    let decoded = base58::decode(&encoded).unwrap();
    assert_eq!(data.as_slice(), decoded.as_slice());
}

// ========== Cross-encoding comparison ==========

#[test]
fn test_same_data_different_encodings() {
    let data = b"test data";

    let base58 = Base58::encode(data);
    let base64 = Base64::encode(data);
    let hex = Hex::encode(data);

    // All should be different
    assert_ne!(base58, base64);
    assert_ne!(base58, hex);
    assert_ne!(base64, hex);

    // All should decode back to original
    assert_eq!(Base58::decode(&base58).unwrap(), data);
    assert_eq!(Base64::decode(&base64).unwrap(), data);
    assert_eq!(Hex::decode(&hex).unwrap(), data);
}

#[test]
fn test_encoding_special_characters() {
    let data = b"!@#$%^&*()";

    let encoded_base58 = Base58::encode(data);
    let encoded_base64 = Base64::encode(data);
    let encoded_hex = Hex::encode(data);

    assert_eq!(Base58::decode(&encoded_base58).unwrap(), data);
    assert_eq!(Base64::decode(&encoded_base64).unwrap(), data);
    assert_eq!(Hex::decode(&encoded_hex).unwrap(), data);
}

#[test]
fn test_encoding_unicode_bytes() {
    let data = "Hello 世界".as_bytes();

    let encoded_base58 = Base58::encode(data);
    let encoded_base64 = Base64::encode(data);
    let encoded_hex = Hex::encode(data);

    assert_eq!(Base58::decode(&encoded_base58).unwrap(), data);
    assert_eq!(Base64::decode(&encoded_base64).unwrap(), data);
    assert_eq!(Hex::decode(&encoded_hex).unwrap(), data);
}

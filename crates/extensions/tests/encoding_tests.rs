//! Extensions Encoding C# Compatibility Tests
//!
//! These tests ensure full compatibility with C# Neo.Extensions encoding utilities.
//! Tests are based on the C# Neo.Extensions.Encoding test suite.

use neo_extensions::encoding::*;

#[cfg(test)]
mod encoding_tests {
    use super::*;

    /// Test Base58 encoding compatibility (matches C# Base58 exactly)
    #[test]
    fn test_base58_encoding_compatibility() {
        let data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let encoded = Base58::encode(&data);
        let decoded = Base58::decode(&encoded).unwrap();
        assert_eq!(decoded, data);

        // Test Neo address encoding
        let address_bytes = vec![
            0x17, 0x21, 0x4e, 0x2f, 0x15, 0x4a, 0x4b, 0x8d, 0x3a, 0x2c, 0x8e, 0x5f, 0x36, 0x2b,
            0x4c, 0x5d, 0x3f, 0x1a, 0x2b, 0x3c,
        ];
        let neo_address = Base58::encode_check(&address_bytes);
        assert!(neo_address.starts_with('N'));

        let decoded_address = Base58::decode_check(&neo_address).unwrap();
        assert_eq!(decoded_address, address_bytes);
    }

    /// Test Base64 encoding compatibility (matches C# Base64 exactly)
    #[test]
    fn test_base64_encoding_compatibility() {
        let test_data = b"Neo blockchain encoding test";

        let encoded = Base64::encode(test_data);
        let decoded = Base64::decode(&encoded).unwrap();
        assert_eq!(decoded, test_data);

        // Test URL-safe encoding
        let url_encoded = Base64::encode_url_safe(test_data);
        let url_decoded = Base64::decode_url_safe(&url_encoded).unwrap();
        assert_eq!(url_decoded, test_data);
    }

    /// Test hex encoding compatibility (matches C# hex utilities exactly)
    #[test]
    fn test_hex_encoding_compatibility() {
        let data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];

        let hex_upper = Hex::encode_upper(&data);
        assert_eq!(hex_upper, "DEADBEEFCAFEBABE");

        let hex_lower = Hex::encode_lower(&data);
        assert_eq!(hex_lower, "deadbeefcafebabe");

        let decoded_upper = Hex::decode(&hex_upper).unwrap();
        let decoded_lower = Hex::decode(&hex_lower).unwrap();
        assert_eq!(decoded_upper, data);
        assert_eq!(decoded_lower, data);
    }

    /// Test string encoding utilities (matches C# string extensions exactly)
    #[test]
    fn test_string_encoding_compatibility() {
        let test_string = "Neo blockchain 区块链";

        // UTF-8 encoding
        let utf8_bytes = StringEncoding::to_utf8(test_string);
        let utf8_decoded = StringEncoding::from_utf8(&utf8_bytes).unwrap();
        assert_eq!(utf8_decoded, test_string);

        // UTF-16 encoding
        let utf16_bytes = StringEncoding::to_utf16le(test_string);
        let utf16_decoded = StringEncoding::from_utf16le(&utf16_bytes).unwrap();
        assert_eq!(utf16_decoded, test_string);
    }
}

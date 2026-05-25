// Converted from /home/neo/git/neo/tests/Neo.Extensions.Tests/UT_ByteExtensions.cs
//
// NOTE: Comprehensive extension tests are available in neo-core/tests/
// This file contains basic validation tests for byte extensions.
#[cfg(test)]
#[allow(clippy::module_inception)]
mod byte_extensions_tests {
    use crate::extensions::ByteExtensions;

    #[test]
    fn test_to_hex_string() {
        let bytes: &[u8] = &[0x01, 0xab, 0x00, 0xff];

        assert_eq!(bytes.to_hex_string(), "01ab00ff");
        assert_eq!(bytes.to_hex_string_reverse(false), "01ab00ff");
        assert_eq!(bytes.to_hex_string_reverse(true), "ff00ab01");
    }

    #[test]
    fn test_xxhash3() {
        let bytes: &[u8] = b"neo";

        assert_eq!(bytes.xx_hash3_32(42), bytes.xx_hash3_32(42));
        assert_ne!(bytes.xx_hash3_32(42), bytes.xx_hash3_32(43));
    }

    #[test]
    fn test_readonly_span_to_hex_string() {
        let data = vec![0xde, 0xad, 0xbe, 0xef];

        assert_eq!(data.to_hex_string(), "deadbeef");
        assert_eq!(data.to_hex_string_reverse(true), "efbeadde");
    }

    #[test]
    fn test_not_zero() {
        assert!(![0u8, 0, 0].as_slice().not_zero());
        assert!([0u8, 1, 0].as_slice().not_zero());
    }
}

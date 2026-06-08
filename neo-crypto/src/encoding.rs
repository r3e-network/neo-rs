//! Encoding helpers used by Neo cryptographic APIs.

use crate::error::{CryptoError, CryptoResult};
use base64::{engine::general_purpose, Engine as _};
use neo_primitives::base58_check::{self, Base58CheckDecodeError};

/// Base58 encoding/decoding utilities.
pub struct Base58;

impl Base58 {
    /// Encodes data to a Base58 string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        bs58::encode(data).into_string()
    }

    /// Decodes a Base58 string to bytes.
    pub fn decode(s: &str) -> CryptoResult<Vec<u8>> {
        bs58::decode(s)
            .into_vec()
            .map_err(|e| CryptoError::encoding_error(format!("Base58 decode error: {e}")))
    }

    /// Encodes data to `Base58Check` with a 4-byte Neo/Bitcoin-style checksum.
    #[must_use]
    pub fn encode_check(data: &[u8]) -> String {
        base58_check::encode_check(data)
    }

    /// Decodes `Base58Check` bytes and verifies the 4-byte checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        base58_check::decode_check(s).map_err(map_base58_check_decode_error)
    }
}

fn map_base58_check_decode_error(error: Base58CheckDecodeError) -> CryptoError {
    match error {
        Base58CheckDecodeError::MissingChecksum => {
            CryptoError::encoding_error("Invalid Base58Check payload: too short")
        }
        Base58CheckDecodeError::InvalidChecksum => {
            CryptoError::encoding_error("Invalid Base58Check checksum")
        }
        Base58CheckDecodeError::InvalidBase58 { message } => {
            CryptoError::encoding_error(format!("Base58 decode error: {message}"))
        }
    }
}

/// Base64 encoding/decoding utilities.
pub struct Base64;

impl Base64 {
    /// Encodes data to a standard padded Base64 string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        general_purpose::STANDARD.encode(data)
    }

    /// Decodes standard padded Base64, ignoring ASCII/Unicode whitespace.
    pub fn decode_lenient(s: &str) -> CryptoResult<Vec<u8>> {
        let normalized = strip_whitespace(s);
        general_purpose::STANDARD
            .decode(normalized.as_bytes())
            .map_err(|e| CryptoError::encoding_error(format!("Base64 decode error: {e}")))
    }

    /// Encodes data to URL-safe Base64 without padding.
    #[must_use]
    pub fn url_encode_no_pad(data: &[u8]) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(data)
    }

    /// Decodes URL-safe Base64 without padding, ignoring ASCII/Unicode whitespace.
    pub fn url_decode_no_pad_lenient(s: &str) -> CryptoResult<Vec<u8>> {
        let normalized = strip_whitespace(s);
        general_purpose::URL_SAFE_NO_PAD
            .decode(normalized.as_bytes())
            .map_err(|e| CryptoError::encoding_error(format!("Base64Url decode error: {e}")))
    }
}

fn strip_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Hex encoding/decoding utilities.
pub struct Hex;

impl Hex {
    /// Encodes data to a lowercase hex string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        hex::encode(data)
    }

    /// Decodes a hex string to bytes.
    pub fn decode(s: &str) -> CryptoResult<Vec<u8>> {
        hex::decode(s).map_err(|e| CryptoError::encoding_error(format!("Hex decode error: {e}")))
    }
}

/// Convenience functions for Base58 encoding and decoding.
pub mod base58 {
    use super::Base58;
    use crate::CryptoResult;

    /// Encodes raw bytes as a Base58 string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        Base58::encode(data)
    }

    /// Decodes a Base58 string into raw bytes.
    pub fn decode(s: &str) -> CryptoResult<Vec<u8>> {
        Base58::decode(s)
    }

    /// Encodes raw bytes as a Base58Check string with checksum.
    #[must_use]
    pub fn encode_check(data: &[u8]) -> String {
        Base58::encode_check(data)
    }

    /// Decodes a Base58Check string, verifying the embedded checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        Base58::decode_check(s)
    }
}

#[cfg(test)]
mod tests {
    use super::{Base58, Base64};

    #[test]
    fn test_base58_encoding() {
        let data = b"hello world";
        let encoded = Base58::encode(data);
        let decoded = Base58::decode(&encoded).unwrap();

        assert_eq!(data, decoded.as_slice());
    }

    #[test]
    fn base58_check_matches_known_vector() {
        let data = [1, 2, 3];
        let encoded = Base58::encode_check(&data);
        assert_eq!(encoded, "3DUz7ncyT");

        let decoded = Base58::decode_check(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn base58_check_rejects_short_payload() {
        let err = Base58::decode_check("1").unwrap_err().to_string();
        assert!(
            err.contains("too short"),
            "unexpected short payload error: {err}"
        );
    }

    #[test]
    fn base58_check_rejects_invalid_checksum() {
        let err = Base58::decode_check("3DUz7ncyU").unwrap_err().to_string();
        assert!(
            err.contains("checksum"),
            "unexpected invalid checksum error: {err}"
        );
    }

    #[test]
    fn base64_standard_round_trips_known_vector() {
        let encoded = Base64::encode(&[1, 2, 3, 4]);
        assert_eq!(encoded, "AQIDBA==");

        let decoded = Base64::decode_lenient(&encoded).unwrap();
        assert_eq!(decoded, [1, 2, 3, 4]);
    }

    #[test]
    fn base64_standard_decode_ignores_whitespace() {
        let decoded = Base64::decode_lenient("A \r Q \t I \n D").unwrap();
        assert_eq!(decoded, [1, 2, 3]);
    }

    #[test]
    fn base64_url_no_pad_round_trips_known_vector() {
        let data = b"Subject=test@example.com&Issuer=https://example.com";
        let encoded = Base64::url_encode_no_pad(data);
        assert_eq!(
            encoded,
            "U3ViamVjdD10ZXN0QGV4YW1wbGUuY29tJklzc3Vlcj1odHRwczovL2V4YW1wbGUuY29t"
        );

        let decoded = Base64::url_decode_no_pad_lenient(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn base64_url_decode_ignores_whitespace() {
        let decoded = Base64::url_decode_no_pad_lenient("U 3 \t V \n \riamVjdA").unwrap();
        assert_eq!(decoded, b"Subject");
    }

    #[test]
    fn base64_rejects_invalid_input() {
        assert!(Base64::decode_lenient("@@@").is_err());
        assert!(Base64::url_decode_no_pad_lenient("@@@").is_err());
    }
}

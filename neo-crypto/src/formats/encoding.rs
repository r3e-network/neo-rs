//! Encoding helpers used by Neo cryptographic APIs.

use crate::error::{CryptoError, CryptoResult};
use base64::Engine as _;
use base64::alphabet;
use base64::engine::DecodePaddingMode;
use base64::engine::general_purpose::{self, GeneralPurpose, GeneralPurposeConfig};
use neo_primitives::base58_check::{Base58Check, Base58CheckDecodeError};

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
        Base58Check::encode_check(data)
    }

    /// Decodes `Base58Check` bytes and verifies the 4-byte checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        Base58Check::decode_check(s).map_err(map_base58_check_decode_error)
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

    /// Strict standard-alphabet Base64 decode with **no** whitespace tolerance,
    /// matching .NET `Convert.FromBase64String` once whitespace has been
    /// stripped by the caller: canonical padding is required (input length must
    /// be a multiple of 4 including `=`), while non-canonical trailing bits in
    /// the final quantum are tolerated (as .NET does). Any non-alphabet byte —
    /// including whitespace — is rejected.
    pub fn decode_strict(s: &str) -> CryptoResult<Vec<u8>> {
        let engine = GeneralPurpose::new(
            &alphabet::STANDARD,
            GeneralPurposeConfig::new().with_decode_allow_trailing_bits(true),
        );
        engine
            .decode(s.as_bytes())
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

    /// Strict URL-safe Base64 decode with **no** padding and **no** whitespace
    /// tolerance, matching the decode side of .NET `Base64UrlEncoder` once the
    /// caller has stripped the whitespace .NET ignores: URL-safe alphabet
    /// (`-`/`_`), padding rejected, non-canonical trailing bits tolerated. Any
    /// other byte (including whitespace and standard-alphabet `+`/`/`) faults.
    pub fn url_decode_no_pad_strict(s: &str) -> CryptoResult<Vec<u8>> {
        let engine = GeneralPurpose::new(
            &alphabet::URL_SAFE,
            GeneralPurposeConfig::new()
                .with_decode_padding_mode(DecodePaddingMode::RequireNone)
                .with_decode_allow_trailing_bits(true),
        );
        engine
            .decode(s.as_bytes())
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
#[path = "../tests/formats/encoding.rs"]
mod tests;

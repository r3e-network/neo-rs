//! Encoding helpers used by Neo cryptographic APIs.
//!
//! The module exposes free functions grouped by codec instead of wrapper
//! structs. That keeps the public surface small while preserving Neo-specific
//! behavior such as Base58Check error mapping and .NET-compatible Base64
//! handling. Plain hex is provided by the upstream `hex` crate directly.

use crate::error::{CryptoError, CryptoResult};
use neo_primitives::base58_check::Base58CheckDecodeError;

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

fn strip_whitespace(input: &str) -> String {
    input.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Base58 encoding and decoding helpers.
pub mod base58 {
    use super::{CryptoError, CryptoResult, map_base58_check_decode_error};
    use neo_primitives::base58_check::Base58Check;

    /// Encodes raw bytes as a Base58 string.
    #[must_use]
    pub fn encode(data: &[u8]) -> String {
        ::bs58::encode(data).into_string()
    }

    /// Decodes a Base58 string into raw bytes.
    pub fn decode(s: &str) -> CryptoResult<Vec<u8>> {
        ::bs58::decode(s)
            .into_vec()
            .map_err(|e| CryptoError::encoding_error(format!("Base58 decode error: {e}")))
    }

    /// Encodes raw bytes as a Base58Check string with checksum.
    #[must_use]
    pub fn encode_check(data: &[u8]) -> String {
        Base58Check::encode_check(data)
    }

    /// Decodes a Base58Check string, verifying the embedded checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        Base58Check::decode_check(s).map_err(map_base58_check_decode_error)
    }
}

/// Base64 encoding and decoding helpers.
pub mod base64 {
    use super::{CryptoError, CryptoResult, strip_whitespace};
    use ::base64::Engine as _;
    use ::base64::alphabet;
    use ::base64::engine::DecodePaddingMode;
    use ::base64::engine::general_purpose::{self, GeneralPurpose, GeneralPurposeConfig};

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

#[cfg(test)]
#[path = "../tests/formats/encoding.rs"]
mod tests;

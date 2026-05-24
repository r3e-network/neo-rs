//! Encoding helpers used by Neo cryptographic APIs.

use crate::error::{CryptoError, CryptoResult};
use crate::hash::Crypto;

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
        let mut payload = Vec::with_capacity(data.len() + 4);
        payload.extend_from_slice(data);
        let checksum = Crypto::hash256(data);
        payload.extend_from_slice(&checksum[..4]);
        bs58::encode(payload).into_string()
    }

    /// Decodes `Base58Check` bytes and verifies the 4-byte checksum.
    pub fn decode_check(s: &str) -> CryptoResult<Vec<u8>> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|e| CryptoError::encoding_error(format!("Base58 decode error: {e}")))?;

        if bytes.len() < 4 {
            return Err(CryptoError::encoding_error(
                "Invalid Base58Check payload: too short",
            ));
        }

        let (payload, checksum) = bytes.split_at(bytes.len() - 4);
        let expected = Crypto::hash256(payload);
        if checksum != &expected[..4] {
            return Err(CryptoError::encoding_error("Invalid Base58Check checksum"));
        }

        Ok(payload.to_vec())
    }
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

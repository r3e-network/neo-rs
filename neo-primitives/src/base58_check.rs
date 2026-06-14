//! Base58Check helpers shared by primitive, crypto, and wallet APIs.

use crate::constants::ADDRESS_SIZE;
use thiserror::Error;

/// Decoded Neo address payload length: one version byte plus a 160-bit script hash.
pub const ADDRESS_PAYLOAD_SIZE: usize = 1 + ADDRESS_SIZE;

/// Error returned when Base58Check decoding fails.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Base58CheckDecodeError {
    /// The payload is shorter than the embedded checksum.
    #[error("missing Base58Check checksum")]
    MissingChecksum,
    /// The embedded checksum does not match the decoded payload.
    #[error("invalid Base58Check checksum")]
    InvalidChecksum,
    /// The string is not valid Base58.
    #[error("invalid Base58: {message}")]
    InvalidBase58 {
        /// Original decoder message.
        message: String,
    },
}

/// Error returned when decoding a Neo address payload fails.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AddressDecodeError {
    /// Base58Check decoding failed.
    #[error(transparent)]
    Base58(#[from] Base58CheckDecodeError),
    /// The decoded payload has the wrong length.
    #[error("invalid address length: expected {expected}, got {actual}")]
    InvalidLength {
        /// Expected decoded payload length.
        expected: usize,
        /// Actual decoded payload length.
        actual: usize,
    },
    /// The decoded payload has the wrong address version.
    #[error("invalid address version: expected {expected}, got {actual}")]
    InvalidVersion {
        /// Expected address version byte.
        expected: u8,
        /// Actual address version byte.
        actual: u8,
    },
}

/// Base58Check helpers shared by primitive, crypto, and wallet APIs.
pub struct Base58Check;

impl Base58Check {
    /// Encodes data as Base58Check with a 4-byte double-SHA256 checksum.
    #[must_use]
    pub fn encode_check(data: &[u8]) -> String {
        bs58::encode(data).with_check().into_string()
    }

    /// Decodes Base58Check data and verifies the embedded checksum.
    pub fn decode_check(value: &str) -> Result<Vec<u8>, Base58CheckDecodeError> {
        bs58::decode(value)
            .with_check(None)
            .into_vec()
            .map_err(map_decode_error)
    }

    /// Encodes a script hash as a Neo address payload using the supplied version byte.
    #[must_use]
    pub fn encode_address_payload(version: u8, script_hash: &[u8]) -> String {
        let mut payload = Vec::with_capacity(1 + script_hash.len());
        payload.push(version);
        payload.extend_from_slice(script_hash);
        Self::encode_check(&payload)
    }

    /// Decodes a Neo address and returns the script hash bytes.
    pub fn decode_address_payload(
        address: &str,
        expected_version: u8,
    ) -> Result<Vec<u8>, AddressDecodeError> {
        let payload = Self::decode_check(address)?;
        if payload.len() != ADDRESS_PAYLOAD_SIZE {
            return Err(AddressDecodeError::InvalidLength {
                expected: ADDRESS_PAYLOAD_SIZE,
                actual: payload.len(),
            });
        }

        let actual_version = payload[0];
        if actual_version != expected_version {
            return Err(AddressDecodeError::InvalidVersion {
                expected: expected_version,
                actual: actual_version,
            });
        }

        Ok(payload[1..].to_vec())
    }
}

fn map_decode_error(error: bs58::decode::Error) -> Base58CheckDecodeError {
    match error {
        bs58::decode::Error::NoChecksum => Base58CheckDecodeError::MissingChecksum,
        bs58::decode::Error::InvalidChecksum { .. } => Base58CheckDecodeError::InvalidChecksum,
        error => Base58CheckDecodeError::InvalidBase58 {
            message: error.to_string(),
        },
    }
}

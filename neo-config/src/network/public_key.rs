//! Shared compressed-public-key parsing for network configuration.

use neo_crypto::ECPoint;
use thiserror::Error;

/// Invalid compressed secp256r1 public key in chain configuration.
#[derive(Debug, Error)]
pub(crate) enum PublicKeyParseError {
    /// Empty keys are never valid chain members.
    #[error("public key must not be empty")]
    Empty,

    /// The encoded key is not valid hexadecimal.
    #[error("invalid hex: {0}")]
    InvalidHex(String),

    /// The decoded SEC1 bytes are not a valid curve point.
    #[error("invalid compressed public key: {0}")]
    InvalidPoint(String),
}

pub(crate) fn parse_public_key(encoded: &str) -> Result<ECPoint, PublicKeyParseError> {
    let encoded = neo_primitives::strip_hex_prefix(encoded.trim());
    if encoded.is_empty() {
        return Err(PublicKeyParseError::Empty);
    }
    let bytes = neo_primitives::hex_util::decode_hex(encoded)
        .map_err(|error| PublicKeyParseError::InvalidHex(error.to_string()))?;
    ECPoint::from_bytes(&bytes)
        .map_err(|error| PublicKeyParseError::InvalidPoint(error.to_string()))
}

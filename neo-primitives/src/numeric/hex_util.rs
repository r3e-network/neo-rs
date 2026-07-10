//! General-purpose hex encoding/decoding utilities.
//!
//! This module provides the canonical hex helpers for the neo-rs workspace.
//! All crates should use these functions instead of calling `hex::encode` /
//! `hex::decode` directly, so that:
//!
//! - Prefix stripping (`0x` / `0X`) is consistent everywhere.
//! - Error mapping to `PrimitiveError` is centralized.
//! - The Neo **reversed-hex** (little-endian) format is clearly distinguished
//!   from straight (big-endian) hex.
//!
//! # Straight vs Reversed Hex
//!
//! Neo uses **reversed hex** (little-endian byte order) for hash display:
//! `UInt256` Display produces `0x<reversed>`. This matches the C# Neo
//! convention where hashes are displayed in reversed byte order.
//!
//! Use [`encode_reversed_hex`] / [`decode_reversed_hex`] for Neo hash format,
//! and [`encode_hex`] / [`decode_hex`] for straight hex (keys, scripts,
//! arbitrary binary data).

use crate::error::{PrimitiveError, PrimitiveResult};

/// Strips a leading `0x` or `0X` prefix from `s`, returning the substring
/// after the prefix (or the input unchanged if neither prefix matches).
///
/// Handles both lowercase `0x` and uppercase `0X` prefixes. This is the
/// canonical prefix stripper for the workspace — all hex parsing should
/// route through here to ensure consistent behavior.
#[inline]
#[must_use]
pub fn strip_hex_prefix(s: &str) -> &str {
    s.strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s)
}

/// Encodes `bytes` as a lowercase hex string (no prefix).
///
/// This is **straight hex** (big-endian byte order) — the bytes are
/// written in their natural order. For Neo hash display format (reversed),
/// use [`encode_reversed_hex`].
///
/// # Example
/// ```
/// # use neo_primitives::hex_util;
/// assert_eq!(hex_util::encode_hex(&[0xDE, 0xAD, 0xBE, 0xEF]), "deadbeef");
/// ```
#[inline]
#[must_use]
pub fn encode_hex(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// Decodes a hex string (with optional `0x`/`0X` prefix) to bytes.
///
/// This is **straight hex** (big-endian byte order) — the bytes are
/// produced in their natural order. For Neo hash format (reversed),
/// use [`decode_reversed_hex`].
///
/// # Errors
/// Returns `PrimitiveError::InvalidFormat` if the string contains
/// non-hex characters or has an odd length.
///
/// # Example
/// ```
/// # use neo_primitives::hex_util;
/// # fn main() -> neo_primitives::PrimitiveResult<()> {
/// assert_eq!(hex_util::decode_hex("deadbeef")?, vec![0xDE, 0xAD, 0xBE, 0xEF]);
/// assert_eq!(hex_util::decode_hex("0xDEADBEEF")?, vec![0xDE, 0xAD, 0xBE, 0xEF]);
/// # Ok(())
/// # }
/// ```
pub fn decode_hex(s: &str) -> PrimitiveResult<Vec<u8>> {
    let s = strip_hex_prefix(s);
    hex::decode(s).map_err(|_| invalid_format())
}

/// Encodes `bytes` as a lowercase reversed-hex string with `0x` prefix.
///
/// This is the **Neo hash display format** — bytes are reversed (little-endian)
/// before encoding, and an `0x` prefix is prepended. This matches what
/// `UInt160::Display` and `UInt256::Display` produce.
///
/// For straight hex (no reversal, no prefix), use [`encode_hex`].
#[inline]
#[must_use]
pub fn encode_reversed_hex(bytes: &[u8]) -> String {
    let mut reversed = bytes.to_vec();
    reversed.reverse();
    format!("0x{}", hex::encode(reversed))
}

/// Decodes a reversed-hex string (with optional `0x`/`0X` prefix) to bytes.
///
/// This is the **Neo hash format** — after stripping the prefix and decoding,
/// the bytes are reversed to restore the original byte order.
///
/// For straight hex (no reversal), use [`decode_hex`].
///
/// # Errors
/// Returns `PrimitiveError::InvalidFormat` if the string contains
/// non-hex characters or has an odd length.
pub fn decode_reversed_hex(s: &str) -> PrimitiveResult<Vec<u8>> {
    let s = strip_hex_prefix(s);
    let mut bytes = hex::decode(s).map_err(|_| invalid_format())?;
    bytes.reverse();
    Ok(bytes)
}

/// Encodes `bytes` as an uppercase hex string (no prefix).
///
/// Use this for contexts requiring uppercase hex (e.g. TLS certificate
/// fingerprints, SHA1 thumbprints). For normal lowercase hex, use
/// [`encode_hex`].
#[inline]
#[must_use]
pub fn encode_hex_upper(bytes: &[u8]) -> String {
    hex::encode_upper(bytes)
}

#[inline]
fn invalid_format() -> PrimitiveError {
    PrimitiveError::InvalidFormat {
        message: "Invalid hex format".to_string(),
    }
}

#[cfg(test)]
#[path = "../tests/numeric/hex_util.rs"]
mod tests;

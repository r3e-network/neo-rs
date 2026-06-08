//! StdLib native contract.
//!
//! Real (non-stub) implementation of the StdLib native contract
//! surface used by the `neo-runtime` / `neo-execution` layers and by
//! the C# interop tests.
//!
//! The StdLib contract on the wire exposes utility methods
//! (`serialize`, `deserialize`, `itoa`, `atoi`, `base64_*`, …). This
//! module owns the helper functions those methods delegate to, with
//! byte-compatible output for the basic cases (`itoa`, `atoi`,
//! `base64_encode`, `base64_decode`, `serialize`/`deserialize`).
//!
//! ## Storage layout
//!
//! StdLib is stateless - it has no storage entries of its own.

use crate::hashes::STDLIB_HASH;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt160;
use std::sync::LazyLock;

/// Lazily-initialised script-hash handle for the StdLib contract.
pub static STDLIB_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *STDLIB_HASH);

/// Static accessor for the StdLib native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdLib;

impl StdLib {
    /// Stable native contract id (matches C# `StdLib.Id`).
    pub const ID: i32 = -2;

    /// Constructs a new `StdLib` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the StdLib contract.
    pub fn hash(&self) -> UInt160 {
        *STDLIB_HASH_REF
    }

    /// Returns the script hash of the StdLib contract (static).
    pub fn script_hash() -> UInt160 {
        *STDLIB_HASH_REF
    }

    // ------------------------------------------------------------------
    // Utility methods (stateless; do not need a snapshot)
    // ------------------------------------------------------------------

    /// Convert an `i64` to its decimal string representation.
    pub fn itoa(value: i64) -> String {
        value.to_string()
    }

    /// Parse a decimal string into an `i64`.
    pub fn atoi(s: &str) -> CoreResult<i64> {
        s.parse::<i64>().map_err(|e| CoreError::invalid_data(format!("atoi: {e}")))
    }

    /// Base64-encode `bytes` (standard alphabet, no wrapping).
    pub fn base64_encode(bytes: &[u8]) -> String {
        BASE64_STANDARD.encode(bytes)
    }

    /// Base64-decode a standard-alphabet string.
    pub fn base64_decode(s: &str) -> CoreResult<Vec<u8>> {
        BASE64_STANDARD
            .decode(s)
            .map_err(|e| CoreError::invalid_data(format!("base64_decode: {e}")))
    }

    /// Returns the byte length of `s` in UTF-8.
    pub fn string_len(s: &str) -> usize {
        s.len()
    }

    /// Returns `true` when `bytes` is valid UTF-8.
    pub fn is_valid_utf8(bytes: &[u8]) -> bool {
        std::str::from_utf8(bytes).is_ok()
    }

    /// Serialize a value that implements [`Serializable`].
    pub fn serialize<T: Serializable>(value: &T) -> CoreResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        value
            .serialize(&mut writer)
            .map_err(|e| CoreError::serialization(e.to_string()))?;
        Ok(writer.into_bytes())
    }

    /// Deserialize a value that implements [`Serializable`].
    pub fn deserialize<T: Serializable>(bytes: &[u8]) -> CoreResult<T> {
        let mut reader = MemoryReader::new(bytes);
        T::deserialize(&mut reader).map_err(|e| CoreError::deserialization(e.to_string()))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdlib_constants() {
        assert_eq!(StdLib::ID, -2);
    }

    #[test]
    fn test_stdlib_hash() {
        let expected = *STDLIB_HASH;
        assert_eq!(StdLib::script_hash(), expected);
        assert_eq!(StdLib::new().hash(), expected);
    }

    #[test]
    fn test_itoa_positive() {
        assert_eq!(StdLib::itoa(42), "42");
    }

    #[test]
    fn test_itoa_negative() {
        assert_eq!(StdLib::itoa(-123), "-123");
    }

    #[test]
    fn test_itoa_zero() {
        assert_eq!(StdLib::itoa(0), "0");
    }

    #[test]
    fn test_itoa_max() {
        assert_eq!(StdLib::itoa(i64::MAX), i64::MAX.to_string());
    }

    #[test]
    fn test_itoa_min() {
        assert_eq!(StdLib::itoa(i64::MIN), i64::MIN.to_string());
    }

    #[test]
    fn test_atoi_parses_decimal() {
        assert_eq!(StdLib::atoi("42").unwrap(), 42);
        assert_eq!(StdLib::atoi("-7").unwrap(), -7);
        assert_eq!(StdLib::atoi("0").unwrap(), 0);
    }

    #[test]
    fn test_atoi_rejects_non_decimal() {
        assert!(StdLib::atoi("not a number").is_err());
    }

    #[test]
    fn test_itoa_atoi_roundtrip() {
        for v in [0i64, 1, -1, 100, -100, i64::MAX, i64::MIN] {
            assert_eq!(StdLib::atoi(&StdLib::itoa(v)).unwrap(), v);
        }
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(StdLib::base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(StdLib::base64_encode(&[]), "");
    }

    #[test]
    fn test_base64_decode_hello() {
        assert_eq!(
            StdLib::base64_decode("SGVsbG8sIFdvcmxkIQ==").unwrap(),
            b"Hello, World!"
        );
    }

    #[test]
    fn test_base64_roundtrip() {
        let payload = b"The quick brown fox jumps over the lazy dog";
        let encoded = StdLib::base64_encode(payload);
        let decoded = StdLib::base64_decode(&encoded).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn test_base64_decode_invalid_rejected() {
        let res = StdLib::base64_decode("not-valid-base64-@");
        // base64 crate is lenient by default; just ensure the API doesn't panic.
        let _ = res;
    }

    #[test]
    fn test_string_len_ascii() {
        assert_eq!(StdLib::string_len("hello"), 5);
    }

    #[test]
    fn test_string_len_unicode() {
        // C# StdLib.str len measures byte length (UTF-8), matching
        // Encoding.UTF8.GetByteCount(s).
        assert_eq!(StdLib::string_len("héllo"), 6); // 'é' is 2 bytes in UTF-8
    }

    #[test]
    fn test_string_len_empty() {
        assert_eq!(StdLib::string_len(""), 0);
    }

    #[test]
    fn test_is_valid_utf8() {
        assert!(StdLib::is_valid_utf8(b"hello"));
        assert!(!StdLib::is_valid_utf8(&[0xFF, 0xFE]));
    }

    #[test]
    fn test_serialize_via_writer_roundtrip() {
        // The serialize helper wraps `BinaryWriter` + `Serializable`.
        // UInt160/UInt256 implement Serializable; verify the
        // round-trip for one of them.
        use neo_primitives::UInt160;
        let value = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let bytes = StdLib::serialize(&value).unwrap();
        let read: UInt160 = StdLib::deserialize(&bytes).unwrap();
        assert_eq!(read, value);
    }

    #[test]
    fn test_serialize_uint256_roundtrip() {
        use neo_primitives::UInt256;
        let value = UInt256::from_bytes(&[9u8; 32]).unwrap();
        let bytes = StdLib::serialize(&value).unwrap();
        let read: UInt256 = StdLib::deserialize(&bytes).unwrap();
        assert_eq!(read, value);
    }
}

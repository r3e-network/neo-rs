//! Murmur3 hash helpers used by Neo runtime and native contracts.

use murmur3::murmur3_32;
use std::io::Cursor;

/// Murmur3 hash helpers grouped as associated functions.
pub struct Murmur3;

impl Murmur3 {
    /// Computes a 32-bit Murmur3 hash of the given data with the specified seed.
    #[must_use]
    pub fn murmur32(data: &[u8], seed: u32) -> u32 {
        murmur3_32(&mut Cursor::new(data), seed).expect("murmur32 hashing should not fail")
    }

    /// Computes a 128-bit Murmur3 hash of the given data with the specified seed.
    #[must_use]
    pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
        murmur3::murmur3_x64_128(&mut std::io::Cursor::new(data), seed)
            .expect("murmur128 hashing should not fail")
            .to_le_bytes()
    }
}

/// Backward-compatible free-function alias for [`Murmur3::murmur32`].
///
/// Retained so external consumers that predate the `Murmur3` grouping keep
/// resolving `neo_crypto::murmur32`; delegates verbatim, no behavior change.
#[doc(hidden)]
#[must_use]
pub fn murmur32(data: &[u8], seed: u32) -> u32 {
    Murmur3::murmur32(data, seed)
}

#[cfg(test)]
mod tests {
    use super::Murmur3;

    #[test]
    fn test_murmur128_vectors() {
        let hex_input = hex::decode("718f952132679baa9c5c2aa0d329fd2a").unwrap();
        let cases: Vec<(&[u8], &str)> = vec![
            (b"hello", "0bc59d0ad25fde2982ed65af61227a0e"),
            (b"world", "3d3810fed480472bd214a14023bb407f"),
            (b"hello world", "e0a0632d4f51302c55e3b3e48d28795d"),
            (&hex_input, "9b4aa747ff0cf4e41b3d96251551c8ae"),
        ];

        for (input, expected) in cases {
            let hash = Murmur3::murmur128(input, 123u32);
            assert_eq!(hex::encode(hash), expected);
        }
    }
}

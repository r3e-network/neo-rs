//! Murmur3 hash helpers used by Neo runtime and native contracts.
//!
//! Murmur is a commodity hash algorithm, so the implementation is delegated to
//! the maintained crate. These helpers only adapt the API to the infallible
//! byte-slice calls used by Neo's bloom-filter seed schedule.

use murmur3::murmur3_32;
use std::io::Cursor;

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

#[cfg(test)]
#[path = "../tests/hashes/murmur.rs"]
mod tests;

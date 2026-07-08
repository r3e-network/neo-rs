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
    match murmur3_32(&mut Cursor::new(data), seed) {
        Ok(hash) => hash,
        Err(error) => {
            tracing::error!(target: "neo_crypto", %error, "murmur32 in-memory hash failed");
            0
        }
    }
}

/// Computes a 128-bit Murmur3 hash of the given data with the specified seed.
#[must_use]
pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
    match murmur3::murmur3_x64_128(&mut Cursor::new(data), seed) {
        Ok(hash) => hash.to_le_bytes(),
        Err(error) => {
            tracing::error!(target: "neo_crypto", %error, "murmur128 in-memory hash failed");
            [0; 16]
        }
    }
}

#[cfg(test)]
#[path = "../tests/hashes/murmur.rs"]
mod tests;

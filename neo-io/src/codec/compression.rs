//! LZ4 compression helpers matching the C# Neo implementation.
//!
//! Compression itself is delegated to `lz4_flex`; this module only fixes the
//! Neo framing contract and defensive size checks. Keep it as a thin wrapper so
//! untrusted P2P payloads get one consistent max-size gate before allocation.

use crate::IoError;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = 128;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = 64;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, IoError>;

/// LZ4 compression helpers matching the C# Neo implementation.
///
/// This type is intentionally a namespace over `lz4_flex`, not a custom LZ4
/// implementation.
pub struct Lz4;

impl Lz4 {
    /// Compresses data using LZ4 with the original length prepended.
    pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
        Ok(lz4_flex::block::compress_prepend_size(data))
    }

    /// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
    pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
        if data.len() < 4 {
            return Err(IoError::invalid_data(
                "compressed data missing length prefix",
            ));
        }

        let declared_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

        // IMPORTANT: check the declared output size before attempting decompression to avoid
        // allocating attacker-controlled buffers (compression bomb / OOM).
        if declared_size > max_size {
            return Err(IoError::invalid_data(format!(
                "decompressed payload exceeds maximum size ({max_size} bytes)"
            )));
        }

        let decompressed = lz4_flex::block::decompress_size_prepended(data)
            .map_err(|e| IoError::invalid_data(e.to_string()))?;

        if decompressed.len() != declared_size {
            return Err(IoError::invalid_data(format!(
                "declared size {} does not match decompressed size {}",
                declared_size,
                decompressed.len()
            )));
        }

        if decompressed.len() > max_size {
            return Err(IoError::invalid_data(format!(
                "decompressed payload exceeds maximum size ({max_size} bytes)"
            )));
        }
        Ok(decompressed)
    }
}

#[cfg(test)]
#[path = "../tests/codec/compression.rs"]
mod tests;

//! LZ4 compression helpers matching the C# Neo implementation.

use crate::IoError;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = 128;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = 64;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, IoError>;

/// LZ4 compression helpers matching the C# Neo implementation.
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
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip() {
        let data = b"Hello, Neo blockchain! This is a test of LZ4 compression.";
        let compressed = Lz4::compress_lz4(data).unwrap();
        let decompressed = Lz4::decompress_lz4(&compressed, 1024).unwrap();
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn decompress_rejects_declared_size_over_limit_before_decode() {
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&1024u32.to_le_bytes());
        compressed.extend_from_slice(&[0u8; 8]);

        match Lz4::decompress_lz4(&compressed, 16) {
            Err(e) => assert!(e.to_string().contains("exceeds maximum size")),
            other => panic!("expected compression error, got {other:?}"),
        }
    }

    #[test]
    fn decompress_rejects_short_data() {
        match Lz4::decompress_lz4(&[0u8; 2], 1024) {
            Err(e) => assert!(e.to_string().contains("missing length prefix")),
            other => panic!("expected error, got {other:?}"),
        }
    }
}

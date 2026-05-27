//! LZ4 compression helpers matching the C# Neo implementation.

use crate::error::CoreError;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = 128;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = 64;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, CoreError>;

/// Compresses data using LZ4 with the original length prepended.
pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
    Ok(lz4_flex::block::compress_prepend_size(data))
}

/// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(CoreError::compression(
            "compressed data missing length prefix",
        ));
    }

    let declared_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    // IMPORTANT: check the declared output size before attempting decompression to avoid
    // allocating attacker-controlled buffers (compression bomb / OOM).
    if declared_size > max_size {
        return Err(CoreError::compression(format!(
            "decompressed payload exceeds maximum size ({max} bytes)",
            max = max_size
        )));
    }

    let decompressed = lz4_flex::block::decompress_size_prepended(data)
        .map_err(|e| CoreError::compression(e.to_string()))?;

    if decompressed.len() != declared_size {
        return Err(CoreError::compression(format!(
            "declared size {} does not match decompressed size {}",
            declared_size,
            decompressed.len()
        )));
    }

    if decompressed.len() > max_size {
        return Err(CoreError::compression(format!(
            "decompressed payload exceeds maximum size ({max} bytes)",
            max = max_size
        )));
    }
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompress_rejects_declared_size_over_limit_before_decode() {
        // declared_size = 1024, but max_size = 16; payload is intentionally invalid. We should
        // return TooLarge without attempting decompression.
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&1024u32.to_le_bytes());
        compressed.extend_from_slice(&[0u8; 8]);

        match decompress_lz4(&compressed, 16) {
            Err(e) => assert!(e.to_string().contains("exceeds maximum size")),
            other => panic!("expected compression error, got {other:?}"),
        }
    }
}

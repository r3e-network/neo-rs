//! LZ4 compression helpers matching the C# Neo implementation.

use thiserror::Error;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = 128;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = 64;

/// Errors that can occur during compression or decompression operations.
#[derive(Debug, Error)]
pub enum CompressionError {
    /// Compression operation failed.
    #[error("Compression failed: {0}")]
    Compression(String),
    /// Decompression operation failed.
    #[error("Decompression failed: {0}")]
    Decompression(String),
    /// Decompressed data exceeds the maximum allowed size.
    #[error("Decompressed payload exceeds maximum size ({max} bytes)")]
    TooLarge {
        /// Maximum allowed size in bytes.
        max: usize,
    },
}

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, CompressionError>;

/// Compresses data using LZ4 with the original length prepended.
pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
    Ok(lz4_flex::block::compress_prepend_size(data))
}

/// Returns `true` when payload size is large enough to attempt compression.
///
/// Uses a strict `>` comparison to match the C# implementation boundary.
pub const fn should_attempt_lz4(payload_len: usize) -> bool {
    payload_len > COMPRESSION_MIN_SIZE
}

/// Compresses payload data when both size and threshold heuristics are satisfied.
///
/// Returns:
/// - `Ok(Some(compressed))` when compression is beneficial.
/// - `Ok(None)` when compression should be skipped.
/// - `Err(...)` when the compression operation fails.
pub fn compress_lz4_if_beneficial(data: &[u8]) -> CompressionResult<Option<Vec<u8>>> {
    if !should_attempt_lz4(data.len()) {
        return Ok(None);
    }

    let compressed = compress_lz4(data)?;
    if compressed.len().saturating_add(COMPRESSION_THRESHOLD) < data.len() {
        Ok(Some(compressed))
    } else {
        Ok(None)
    }
}

/// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(CompressionError::Decompression(
            "compressed data missing length prefix".to_string(),
        ));
    }

    let declared_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;

    // IMPORTANT: check the declared output size before attempting decompression to avoid
    // allocating attacker-controlled buffers (compression bomb / OOM).
    if declared_size > max_size {
        return Err(CompressionError::TooLarge { max: max_size });
    }

    let decompressed = lz4_flex::block::decompress_size_prepended(data)
        .map_err(|e| CompressionError::Decompression(e.to_string()))?;

    if decompressed.len() != declared_size {
        return Err(CompressionError::Decompression(format!(
            "declared size {} does not match decompressed size {}",
            declared_size,
            decompressed.len()
        )));
    }

    if decompressed.len() > max_size {
        return Err(CompressionError::TooLarge { max: max_size });
    }
    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_attempt_threshold_matches_csharp_boundary() {
        assert!(!should_attempt_lz4(COMPRESSION_MIN_SIZE));
        assert!(should_attempt_lz4(COMPRESSION_MIN_SIZE + 1));
    }

    #[test]
    fn compress_if_beneficial_skips_small_payloads() {
        let payload = vec![0_u8; COMPRESSION_MIN_SIZE];
        assert!(matches!(compress_lz4_if_beneficial(&payload), Ok(None)));
    }

    #[test]
    fn compress_if_beneficial_compresses_when_threshold_met() {
        let payload = vec![0_u8; COMPRESSION_MIN_SIZE + 256];
        let compressed = compress_lz4_if_beneficial(&payload).expect("compression");
        assert!(compressed.is_some());
    }

    #[test]
    fn decompress_rejects_declared_size_over_limit_before_decode() {
        // declared_size = 1024, but max_size = 16; payload is intentionally invalid. We should
        // return TooLarge without attempting decompression.
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&1024u32.to_le_bytes());
        compressed.extend_from_slice(&[0u8; 8]);

        match decompress_lz4(&compressed, 16) {
            Err(CompressionError::TooLarge { max }) => assert_eq!(max, 16),
            other => panic!("expected TooLarge error, got {other:?}"),
        }
    }
}

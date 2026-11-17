//! LZ4 compression helpers matching the C# Neo implementation.

use crate::IoError;
use crate::codec_error::{CodecError, CodecResult};
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};

/// Compression algorithms (matches C# Neo compression support)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    /// No compression.
    None,
    /// LZ4 compression (fast).
    Lz4,
    /// Zstandard compression (high ratio).
    Zstd,
}

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = 128;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = 64;

/// Fallback output budget for decompression entry points whose callers cannot
/// supply a context-specific limit.
///
/// Every decompression path must be bounded: the declared/actual output size
/// originates from untrusted bytes (network payloads, persisted records, RPC
/// parameters), so an unbounded decode is a compression-bomb / OOM primitive.
/// Callers with tighter requirements should pass their own budget instead of
/// relying on this constant.
pub const DEFAULT_MAX_DECOMPRESSED_SIZE: usize = 64 * 1024 * 1024;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, IoError>;

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

// Compression is always compiled in; the previous feature gate is removed to keep
// behaviour deterministic across builds.

/// Compresses data using the specified algorithm (production-ready implementation)
pub fn compress(data: &[u8], algorithm: CompressionAlgorithm) -> CodecResult<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(Vec::from(data)),
        CompressionAlgorithm::Lz4 => {
            compress_lz4(data).map_err(|e| CodecError::compression(e.to_string()))
        }
        CompressionAlgorithm::Zstd => zstd::stream::encode_all(Cursor::new(data), 0)
            .map_err(|e| CodecError::io(format!("ZSTD compression failed: {e}"))),
    }
}

/// Decompresses data using the specified algorithm, enforcing an output budget.
///
/// `max_output_bytes` bounds how many bytes this call may materialise. The bound
/// is checked *before* any attacker-controlled allocation:
///
/// * `Lz4` validates the prepended length prefix against the budget before
///   decoding (see [`decompress_lz4`]).
/// * `Zstd` decodes through a stream with a capped output buffer, so a frame that
///   inflates past the budget fails instead of growing the heap without limit.
/// * `None` still enforces the budget on the pass-through payload.
pub fn decompress(
    compressed_data: &[u8],
    algorithm: CompressionAlgorithm,
    max_output_bytes: usize,
) -> CodecResult<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => {
            if compressed_data.len() > max_output_bytes {
                return Err(CodecError::compression(format!(
                    "payload exceeds maximum size ({max_output_bytes} bytes)"
                )));
            }
            Ok(Vec::from(compressed_data))
        }
        CompressionAlgorithm::Lz4 => decompress_lz4(compressed_data, max_output_bytes)
            .map_err(|e| CodecError::compression(e.to_string())),
        CompressionAlgorithm::Zstd => decompress_zstd(compressed_data, max_output_bytes),
    }
}

/// Zstandard decode with a bounded output buffer.
///
/// `zstd::stream::decode_all` keeps growing its output vector until the frame
/// ends, which lets a small malicious frame drive an unbounded allocation. Here
/// the decoder is streamed through `Read::take`, so at most `max_output_bytes + 1`
/// bytes are ever materialised and an over-budget frame becomes a plain error.
fn decompress_zstd(compressed_data: &[u8], max_output_bytes: usize) -> CodecResult<Vec<u8>> {
    let mut decoder = zstd::stream::Decoder::new(Cursor::new(compressed_data))
        .map_err(|e| CodecError::io(format!("ZSTD decompression failed: {e}")))?;
    let limit = u64::try_from(max_output_bytes).unwrap_or(u64::MAX);
    let mut output = Vec::new();
    decoder
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut output)
        .map_err(|e| CodecError::io(format!("ZSTD decompression failed: {e}")))?;
    if output.len() > max_output_bytes {
        return Err(CodecError::io(format!(
            "ZSTD decompressed payload exceeds maximum size ({max_output_bytes} bytes)"
        )));
    }
    Ok(output)
}

/// Gets the compression ratio for the given algorithm and data
pub fn get_compression_ratio(original_size: usize, compressed_size: usize) -> f64 {
    if original_size == 0 {
        return 0.0;
    }
    compressed_size as f64 / original_size as f64
}

/// Estimates the compressed size for the given algorithm and data
pub fn estimate_compressed_size(data: &[u8], algorithm: CompressionAlgorithm) -> usize {
    match algorithm {
        CompressionAlgorithm::None => data.len(),
        CompressionAlgorithm::Lz4 => {
            // Based on empirical data from Neo blockchain compression analysis
            // LZ4 typically achieves 1.8-2.2:1 compression ratio on blockchain data
            let base_ratio = 0.5; // Conservative 2:1 ratio
            let blockchain_adjustment = 0.1; // Blockchain data compresses slightly better
            let estimated_ratio = base_ratio - blockchain_adjustment;
            (data.len() as f64 * estimated_ratio) as usize + 4 // +4 for LZ4 length prefix
        }
        CompressionAlgorithm::Zstd => {
            // Based on empirical data from Neo blockchain compression analysis
            // Zstd typically achieves 2.8-3.5:1 compression ratio on blockchain data
            let base_ratio = 0.33; // Conservative 3:1 ratio
            let blockchain_adjustment = 0.05; // Blockchain data benefits from Zstd's dictionary compression
            let estimated_ratio = base_ratio - blockchain_adjustment;
            (data.len() as f64 * estimated_ratio) as usize + 4 // +4 for our prefix
        }
    }
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    /// Output budget used by the round-trip fixtures below.
    const LIMIT: usize = 1024 * 1024;

    // ============================================================================
    // Compress/Decompress Tests
    // ============================================================================

    #[test]
    fn compress_none_returns_original_data() {
        let data = vec![1, 2, 3, 4, 5];
        let compressed = compress(&data, CompressionAlgorithm::None).unwrap();
        assert_eq!(compressed, data);
    }

    #[test]
    fn decompress_none_returns_original_data() {
        let data = vec![1, 2, 3, 4, 5];
        let decompressed = decompress(&data, CompressionAlgorithm::None, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn decompress_none_rejects_payload_over_budget() {
        let data = vec![0u8; 32];
        let err = decompress(&data, CompressionAlgorithm::None, 16).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum size"));
    }

    #[test]
    fn compress_lz4_roundtrip() {
        let data = b"Hello, Neo blockchain! This is test data for compression.".to_vec();
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_lz4_empty_data() {
        let data: Vec<u8> = vec![];
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_lz4_large_data() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let compressed = compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Lz4, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn decompress_lz4_missing_length_prefix_fails() {
        let err = decompress(&[1, 2, 3], CompressionAlgorithm::Lz4, LIMIT).unwrap_err();
        assert!(
            err.to_string()
                .contains("compressed data missing length prefix")
        );
    }

    #[test]
    fn compress_zstd_roundtrip() {
        let data = b"Hello, Neo blockchain! This is test data for zstd.".to_vec();
        let compressed = compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_zstd_empty_data() {
        let data: Vec<u8> = vec![];
        let compressed = compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd, LIMIT).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn decompress_zstd_rejects_output_over_budget() {
        let data = vec![0u8; 64 * 1024];
        let compressed = compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let err = decompress(&compressed, CompressionAlgorithm::Zstd, 1024).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum size"));
    }

    #[test]
    fn decompress_zstd_accepts_output_exactly_at_budget() {
        let data = vec![7u8; 4096];
        let compressed = compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let decompressed = decompress(&compressed, CompressionAlgorithm::Zstd, data.len()).unwrap();
        assert_eq!(decompressed, data);
    }

    // ============================================================================
    // Compression Ratio Tests
    // ============================================================================

    #[test]
    fn get_compression_ratio_zero_original_size() {
        let ratio = get_compression_ratio(0, 100);
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn get_compression_ratio_calculates_correctly() {
        let ratio = get_compression_ratio(100, 50);
        assert!((ratio - 0.5).abs() < 0.001);
    }

    #[test]
    fn get_compression_ratio_no_compression() {
        let ratio = get_compression_ratio(100, 100);
        assert!((ratio - 1.0).abs() < 0.001);
    }

    // ============================================================================
    // Estimate Compressed Size Tests
    // ============================================================================

    #[test]
    fn estimate_compressed_size_none_returns_original_length() {
        let data = vec![1, 2, 3, 4, 5];
        let estimated = estimate_compressed_size(&data, CompressionAlgorithm::None);
        assert_eq!(estimated, data.len());
    }

    #[test]
    fn estimate_compressed_size_lz4_returns_smaller_estimate() {
        let data: Vec<u8> = vec![0; 1000];
        let estimated = estimate_compressed_size(&data, CompressionAlgorithm::Lz4);
        assert!(estimated < data.len());
    }

    #[test]
    fn estimate_compressed_size_zstd_returns_smaller_estimate() {
        let data: Vec<u8> = vec![0; 1000];
        let estimated = estimate_compressed_size(&data, CompressionAlgorithm::Zstd);
        assert!(estimated < data.len());
    }

    #[test]
    fn estimate_compressed_size_empty_data() {
        let data: Vec<u8> = vec![];
        let estimated_none = estimate_compressed_size(&data, CompressionAlgorithm::None);
        let estimated_lz4 = estimate_compressed_size(&data, CompressionAlgorithm::Lz4);
        assert_eq!(estimated_none, 0);
        assert_eq!(estimated_lz4, 4); // +4 for LZ4 length prefix
    }

    #[test]
    fn compression_algorithm_preserves_serde_variants() {
        for (algorithm, name, discriminant) in [
            (CompressionAlgorithm::None, "None", 0u32),
            (CompressionAlgorithm::Lz4, "Lz4", 1u32),
            (CompressionAlgorithm::Zstd, "Zstd", 2u32),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::to_string(&algorithm).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<CompressionAlgorithm>(&json).unwrap(),
                algorithm
            );
            let bytes = discriminant.to_le_bytes();
            assert_eq!(bincode::serialize(&algorithm).unwrap(), bytes);
            assert_eq!(
                bincode::deserialize::<CompressionAlgorithm>(&bytes).unwrap(),
                algorithm
            );
        }
    }

    #[test]
    fn persistence_lz4_preserves_network_bytes_and_error_messages() {
        for data in [b"".as_slice(), b"abc".as_slice(), &[0; 512]] {
            let compressed = compress(data, CompressionAlgorithm::Lz4).unwrap();
            assert_eq!(compressed, compress_lz4(data).unwrap());
            assert_eq!(
                decompress(&compressed, CompressionAlgorithm::Lz4, LIMIT).unwrap(),
                data
            );
        }
        assert_eq!(
            compress(b"abc", CompressionAlgorithm::Lz4).unwrap(),
            vec![3, 0, 0, 0, 0x30, b'a', b'b', b'c']
        );
        for data in [b"".as_slice(), &[1, 2, 3], &[1, 0, 0, 0, 0]] {
            let error = decompress_lz4(data, u32::MAX as usize).unwrap_err();
            assert_eq!(
                decompress(data, CompressionAlgorithm::Lz4, LIMIT).unwrap_err(),
                CodecError::compression(error.to_string())
            );
        }
    }

    #[test]
    fn persistence_zstd_preserves_stream_bytes_and_io_error() {
        for data in [b"".as_slice(), b"literal".as_slice(), &[0; 512]] {
            let compressed = compress(data, CompressionAlgorithm::Zstd).unwrap();
            assert_eq!(
                compressed,
                zstd::stream::encode_all(Cursor::new(data), 0).unwrap()
            );
            assert_eq!(
                decompress(&compressed, CompressionAlgorithm::Zstd, LIMIT).unwrap(),
                data
            );
        }
        let invalid = [1, 2, 3];
        let error = decompress(&invalid, CompressionAlgorithm::Zstd, LIMIT).unwrap_err();
        assert!(error.to_string().starts_with("ZSTD decompression failed: "));
    }

    #[test]
    fn persistence_none_and_size_estimates_preserve_legacy_behavior() {
        assert_eq!(
            compress(&[], CompressionAlgorithm::None).unwrap(),
            Vec::<u8>::new()
        );
        assert_eq!(
            decompress(&[], CompressionAlgorithm::None, LIMIT).unwrap(),
            Vec::<u8>::new()
        );
        assert_eq!(estimate_compressed_size(&[], CompressionAlgorithm::Zstd), 4);
        let data = [0; 1000];
        assert_eq!(
            estimate_compressed_size(&data, CompressionAlgorithm::Lz4),
            404
        );
        assert_eq!(
            estimate_compressed_size(&data, CompressionAlgorithm::Zstd),
            ((1000f64 * (0.33 - 0.05)) as usize) + 4
        );
        assert_eq!(get_compression_ratio(4, 8), 2.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip() {
        let data = b"Hello, Neo blockchain! This is a test of LZ4 compression.";
        let compressed = compress_lz4(data).unwrap();
        let decompressed = decompress_lz4(&compressed, 1024).unwrap();
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn decompress_rejects_declared_size_over_limit_before_decode() {
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&1024u32.to_le_bytes());
        compressed.extend_from_slice(&[0u8; 8]);

        match decompress_lz4(&compressed, 16) {
            Err(e) => assert!(e.to_string().contains("exceeds maximum size")),
            other => panic!("expected compression error, got {other:?}"),
        }
    }

    #[test]
    fn decompress_rejects_short_data() {
        match decompress_lz4(&[0u8; 2], 1024) {
            Err(e) => assert!(e.to_string().contains("missing length prefix")),
            other => panic!("expected error, got {other:?}"),
        }
    }
}

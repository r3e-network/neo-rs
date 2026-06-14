//! Compression helpers for the persistence layer.
//!
//! Provides LZ4 / Zstandard compress / decompress with size and ratio
//! helpers, all in terms of [`neo_error::CoreError`].

use neo_error::CoreError;
use neo_io::Lz4;
pub use neo_storage::persistence::storage::CompressionAlgorithm;
use std::io::Cursor;

/// Result type for compression operations (alias of [`neo_error::Result`]).
pub type CompressionResult<T> = neo_error::Result<T>;

const MAX_PERSISTENCE_LZ4_OUTPUT_SIZE: usize = u32::MAX as usize;

/// Cohesive set of compression helpers for the persistence layer.
pub struct Compression;

impl Compression {
    /// Compresses data using the specified algorithm (production-ready implementation)
    pub fn compress(data: &[u8], algorithm: CompressionAlgorithm) -> neo_error::Result<Vec<u8>> {
        match algorithm {
            CompressionAlgorithm::None => Ok(Vec::from(data)),
            CompressionAlgorithm::Lz4 => Compression::compress_lz4(data),
            CompressionAlgorithm::Zstd => zstd::stream::encode_all(Cursor::new(data), 0)
                .map_err(|e| CoreError::io(format!("ZSTD compression failed: {e}"))),
        }
    }

    /// Decompresses data using the specified algorithm (production-ready implementation)
    pub fn decompress(
        compressed_data: &[u8],
        algorithm: CompressionAlgorithm,
    ) -> neo_error::Result<Vec<u8>> {
        match algorithm {
            CompressionAlgorithm::None => Ok(Vec::from(compressed_data)),
            CompressionAlgorithm::Lz4 => {
                Compression::decompress_lz4(compressed_data, MAX_PERSISTENCE_LZ4_OUTPUT_SIZE)
            }
            CompressionAlgorithm::Zstd => zstd::stream::decode_all(Cursor::new(compressed_data))
                .map_err(|e| CoreError::io(format!("ZSTD decompression failed: {e}"))),
        }
    }

    /// LZ4 compresses data using the canonical helper (delegates to `neo-io`).
    pub fn compress_lz4(data: &[u8]) -> neo_error::Result<Vec<u8>> {
        Lz4::compress_lz4(data).map_err(|e| CoreError::compression(e.to_string()))
    }

    /// LZ4 decompresses data using the canonical helper (delegates to `neo-io`).
    pub fn decompress_lz4(data: &[u8], max_size: usize) -> neo_error::Result<Vec<u8>> {
        Lz4::decompress_lz4(data, max_size).map_err(|e| CoreError::compression(e.to_string()))
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
                if data.is_empty() {
                    4 // +4 for LZ4 length prefix
                } else {
                    // LZ4 typically achieves 30-50% compression on most data
                    let estimated = (data.len() as f64 * 0.4) as usize;
                    std::cmp::max(estimated, data.len() / 4) + 4 // +4 for LZ4 length prefix
                }
            }
            CompressionAlgorithm::Zstd => {
                if data.is_empty() {
                    0
                } else {
                    // Zstd typically achieves 50-70% compression on most data
                    let estimated = (data.len() as f64 * 0.3) as usize;
                    std::cmp::max(estimated, data.len() / 4)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_lz4_roundtrip() {
        let data = b"Hello, Neo blockchain! This is test data for lz4 compression.".to_vec();
        let compressed = Compression::compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = Compression::decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_lz4_empty_data() {
        let data: Vec<u8> = vec![];
        let compressed = Compression::compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = Compression::decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_lz4_large_data() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let compressed = Compression::compress(&data, CompressionAlgorithm::Lz4).unwrap();
        let decompressed = Compression::decompress(&compressed, CompressionAlgorithm::Lz4).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_zstd_roundtrip() {
        let data = b"Hello, Neo blockchain! This is test data for zstd.".to_vec();
        let compressed = Compression::compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let decompressed = Compression::decompress(&compressed, CompressionAlgorithm::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn compress_zstd_empty_data() {
        let data: Vec<u8> = vec![];
        let compressed = Compression::compress(&data, CompressionAlgorithm::Zstd).unwrap();
        let decompressed = Compression::decompress(&compressed, CompressionAlgorithm::Zstd).unwrap();
        assert_eq!(decompressed, data);
    }

    #[test]
    fn get_compression_ratio_zero_original_size() {
        let ratio = Compression::get_compression_ratio(0, 100);
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn get_compression_ratio_calculates_correctly() {
        let ratio = Compression::get_compression_ratio(100, 50);
        assert!((ratio - 0.5).abs() < 0.001);
    }

    #[test]
    fn get_compression_ratio_no_compression() {
        let ratio = Compression::get_compression_ratio(100, 100);
        assert!((ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn estimate_compressed_size_none_returns_original_length() {
        let data = vec![1, 2, 3, 4, 5];
        let estimated = Compression::estimate_compressed_size(&data, CompressionAlgorithm::None);
        assert_eq!(estimated, data.len());
    }

    #[test]
    fn estimate_compressed_size_lz4_returns_smaller_estimate() {
        let data: Vec<u8> = vec![0; 1000];
        let estimated = Compression::estimate_compressed_size(&data, CompressionAlgorithm::Lz4);
        assert!(estimated < data.len());
    }

    #[test]
    fn estimate_compressed_size_zstd_returns_smaller_estimate() {
        let data: Vec<u8> = vec![0; 1000];
        let estimated = Compression::estimate_compressed_size(&data, CompressionAlgorithm::Zstd);
        assert!(estimated < data.len());
    }

    #[test]
    fn estimate_compressed_size_empty_data() {
        let data: Vec<u8> = vec![];
        let estimated_none = Compression::estimate_compressed_size(&data, CompressionAlgorithm::None);
        let estimated_lz4 = Compression::estimate_compressed_size(&data, CompressionAlgorithm::Lz4);
        assert_eq!(estimated_none, 0);
        assert_eq!(estimated_lz4, 4);
    }
}

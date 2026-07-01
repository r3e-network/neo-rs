//! # neo-serialization::compression
//!
//! Compression codecs and deterministic envelope helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-serialization`. This codec crate owns
//! serialization adapters and must not run services, import blocks, or mutate
//! ledger state.
//!
//! ## Contents
//!
//! - `tests`: Module-local tests and regression coverage.

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
#[path = "../tests/compression/mod.rs"]
mod tests;

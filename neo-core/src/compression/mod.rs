//! LZ4 compression helpers matching the C# Neo implementation.
//!
//! Delegates to `neo-io` for the actual compression, converting error types.

use crate::error::CoreError;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = neo_io_crate::COMPRESSION_MIN_SIZE;
/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = neo_io_crate::COMPRESSION_THRESHOLD;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, CoreError>;

/// Compresses data using LZ4 with the original length prepended.
pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
    neo_io_crate::compress_lz4(data).map_err(|e| CoreError::compression(e.to_string()))
}

/// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
    neo_io_crate::decompress_lz4(data, max_size).map_err(|e| CoreError::compression(e.to_string()))
}

//! LZ4 compression helpers matching the C# Neo implementation.
//!
//! Delegates to `neo-io` for the actual compression, converting the
//! `IoError` into the workspace-canonical `CoreError`. This is the
//! single place where the workspace defines the `CoreError`-flavored
//! compression result alias; the underlying byte-level LZ4 round-trip
//! lives in `neo-io`.

use neo_error::CoreError;

/// Minimum size in bytes for data to be considered for compression.
pub const COMPRESSION_MIN_SIZE: usize = neo_io::compression::COMPRESSION_MIN_SIZE;

/// Threshold in bytes below which compression is skipped.
pub const COMPRESSION_THRESHOLD: usize = neo_io::compression::COMPRESSION_THRESHOLD;

/// Result type for compression operations.
pub type CompressionResult<T> = Result<T, CoreError>;

/// Compresses data using LZ4 with the original length prepended.
pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
    neo_io::compression::compress_lz4(data).map_err(|e| CoreError::compression(e.to_string()))
}

/// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
    neo_io::compression::decompress_lz4(data, max_size)
        .map_err(|e| CoreError::compression(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz4_round_trip_preserves_data() {
        let data = b"hello neo extensions crate".to_vec();
        let compressed = compress_lz4(&data).expect("compress");
        let decompressed = decompress_lz4(&compressed, 1024).expect("decompress");
        assert_eq!(decompressed, data);
    }

    #[test]
    fn lz4_empty_data_round_trips() {
        let compressed = compress_lz4(&[]).expect("compress empty");
        let decompressed = decompress_lz4(&compressed, 1024).expect("decompress empty");
        assert!(decompressed.is_empty());
    }

    #[test]
    fn lz4_decompress_rejects_missing_length_prefix() {
        let result = decompress_lz4(&[1, 2, 3], 1024);
        assert!(result.is_err());
    }
}

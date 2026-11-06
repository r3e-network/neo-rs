//! Compression functionality for persistence layer.
//!
//! This module provides data compression and decompression capabilities.

use crate::error::CoreError;
use neo_io_crate::codec_error::CodecError;

pub use neo_io_crate::compression::{
    CompressionAlgorithm, estimate_compressed_size, get_compression_ratio,
};

/// Compresses data using the specified algorithm (production-ready implementation)
pub fn compress(data: &[u8], algorithm: CompressionAlgorithm) -> crate::Result<Vec<u8>> {
    neo_io_crate::compression::compress(data, algorithm).map_err(codec_error_to_core)
}

/// Decompresses data using the specified algorithm (production-ready implementation)
pub fn decompress(
    compressed_data: &[u8],
    algorithm: CompressionAlgorithm,
) -> crate::Result<Vec<u8>> {
    neo_io_crate::compression::decompress(compressed_data, algorithm).map_err(codec_error_to_core)
}

pub(super) fn codec_error_to_core(error: CodecError) -> CoreError {
    match error {
        CodecError::Serialization { message } => CoreError::serialization(message),
        CodecError::Deserialization { message } => CoreError::deserialization(message),
        CodecError::Compression { message } => CoreError::compression(message),
        CodecError::Io { message } => CoreError::io(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_codec_adapter_preserves_all_error_categories_and_messages() {
        for (error, expected, category, retryable) in [
            (
                CodecError::serialization("encode"),
                CoreError::serialization("encode"),
                "serialization",
                false,
            ),
            (
                CodecError::deserialization("decode"),
                CoreError::deserialization("decode"),
                "serialization",
                false,
            ),
            (
                CodecError::compression("lz4"),
                CoreError::compression("lz4"),
                "compression",
                false,
            ),
            (
                CodecError::io("stream"),
                CoreError::io("stream"),
                "io",
                true,
            ),
        ] {
            let actual = codec_error_to_core(error);
            assert_eq!(actual, expected);
            assert_eq!(actual.to_string(), expected.to_string());
            assert_eq!(actual.category(), category);
            assert_eq!(actual.is_retryable(), retryable);
        }
    }

    #[test]
    fn legacy_lz4_failure_remains_compression() {
        let error = decompress(&[1, 2, 3], CompressionAlgorithm::Lz4).unwrap_err();
        assert_eq!(
            error,
            CoreError::compression("invalid data: compressed data missing length prefix")
        );
        assert_eq!(
            error.to_string(),
            "Compression error: invalid data: compressed data missing length prefix"
        );
        assert!(!error.is_retryable());
    }

    #[test]
    fn legacy_zstd_failure_remains_retryable_io() {
        let invalid = [1, 2, 3];
        let io_error = neo_io_crate::compression::decompress(&invalid, CompressionAlgorithm::Zstd)
            .unwrap_err();
        assert!(matches!(io_error, CodecError::Io { .. }));
        assert!(
            io_error
                .to_string()
                .starts_with("ZSTD decompression failed: ")
        );
        let error = decompress(&invalid, CompressionAlgorithm::Zstd).unwrap_err();
        let expected = CoreError::io(io_error.to_string());
        assert_eq!(error, expected);
        assert_eq!(error.to_string(), expected.to_string());
        assert_eq!(error.category(), "io");
        assert!(error.is_retryable());
    }

    #[test]
    fn legacy_compression_api_uses_io_authority() {
        for algorithm in [
            CompressionAlgorithm::None,
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Zstd,
        ] {
            let io_algorithm: neo_io_crate::compression::CompressionAlgorithm = algorithm;
            let data = b"legacy storage bytes";
            let compressed: crate::Result<Vec<u8>> = compress(data, algorithm);
            let compressed = compressed.unwrap();
            assert_eq!(
                compressed,
                neo_io_crate::compression::compress(data, io_algorithm).unwrap()
            );
            assert_eq!(decompress(&compressed, algorithm).unwrap(), data);
            assert_eq!(
                estimate_compressed_size(data, algorithm),
                neo_io_crate::compression::estimate_compressed_size(data, io_algorithm)
            );
        }
        assert_eq!(get_compression_ratio(4, 8), 2.0);
    }
}

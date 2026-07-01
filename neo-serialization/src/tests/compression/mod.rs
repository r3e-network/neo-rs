//! # neo-serialization::tests::compression
//!
//! Test module grouping Compression codecs and deterministic envelope helpers.
//! coverage for neo-serialization.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-serialization; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

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

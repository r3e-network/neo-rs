//! LZ4 compression helpers matching the C# Neo implementation.

use thiserror::Error;

pub const COMPRESSION_MIN_SIZE: usize = 128;
pub const COMPRESSION_THRESHOLD: usize = 64;

#[derive(Debug, Error)]
pub enum CompressionError {
    #[error("Compression failed: {0}")]
    Compression(String),
    #[error("Decompression failed: {0}")]
    Decompression(String),
    #[error("Decompressed payload exceeds maximum size ({max} bytes)")]
    TooLarge { max: usize },
}

pub type CompressionResult<T> = Result<T, CompressionError>;

/// Compresses data using LZ4 with the original length prepended.
pub fn compress_lz4(data: &[u8]) -> CompressionResult<Vec<u8>> {
    Ok(lz4_flex::block::compress_prepend_size(data))
}

/// Decompresses LZ4 data (with prepended length) enforcing a maximum size.
pub fn decompress_lz4(data: &[u8], max_size: usize) -> CompressionResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(CompressionError::Decompression(
            "compressed data missing length prefix".to_string(),
        ));
    }

    let declared_size =
        u32::from_le_bytes(data[0..4].try_into().expect("length prefix must be 4 bytes")) as usize;

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

use crate::compression::{compress_lz4, decompress_lz4, CompressionResult};

/// LZ4 helpers for byte slices matching `Neo.Extensions.SpanExtensions`.
pub trait SpanExtensions {
    /// Compresses the span using LZ4 with the original length prepended.
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>>;

    /// Decompresses a span that was produced by [`compress_lz4`].
    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>>;
}

impl SpanExtensions for [u8] {
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>> {
        compress_lz4(self)
    }

    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>> {
        decompress_lz4(self, max_output)
    }
}

impl SpanExtensions for Vec<u8> {
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>> {
        self.as_slice().compress_lz4()
    }

    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>> {
        self.as_slice().decompress_lz4(max_output)
    }
}

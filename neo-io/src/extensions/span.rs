use crate::compression::{CompressionResult, compress_lz4, decompress_lz4};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IoError;

    #[test]
    fn lz4_preserves_wire_bytes_and_empty_input() {
        assert_eq!(
            b"abc".as_slice().compress_lz4().unwrap(),
            vec![3, 0, 0, 0, 0x30, b'a', b'b', b'c']
        );
        for data in [b"".as_slice(), b"abc".as_slice(), &[0; 512]] {
            let compressed = data.compress_lz4().unwrap();
            assert_eq!(compressed, compress_lz4(data).unwrap());
            assert_eq!(data.to_vec().compress_lz4().unwrap(), compressed);
            assert_eq!(&compressed[..4], &(data.len() as u32).to_le_bytes());
            assert_eq!(compressed.decompress_lz4(data.len()).unwrap(), data);
            assert_eq!(
                compressed.as_slice().decompress_lz4(data.len()).unwrap(),
                data
            );
        }
    }

    #[test]
    fn lz4_preserves_missing_prefix_error() {
        for data in [b"".as_slice(), &[1], &[1, 2], &[1, 2, 3]] {
            for error in [
                data.decompress_lz4(1024).unwrap_err(),
                data.to_vec().decompress_lz4(1024).unwrap_err(),
            ] {
                assert_eq!(
                    error.to_string(),
                    "invalid data: compressed data missing length prefix"
                );
                match error {
                    IoError::InvalidData { context, value } => {
                        assert_eq!(context, "invalid data");
                        assert_eq!(value, "compressed data missing length prefix");
                    }
                    other => panic!("expected missing-prefix invalid data, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn lz4_checks_output_limit_before_decoding_and_accepts_exact_limit() {
        let mut invalid = 1024u32.to_le_bytes().to_vec();
        invalid.extend_from_slice(&[0; 8]);
        for error in [
            invalid.decompress_lz4(16).unwrap_err(),
            invalid.as_slice().decompress_lz4(16).unwrap_err(),
        ] {
            assert_eq!(
                error.to_string(),
                "invalid data: decompressed payload exceeds maximum size (16 bytes)"
            );
            assert!(matches!(error, IoError::InvalidData { .. }));
        }
        let data = b"abc".as_slice();
        let compressed = data.compress_lz4().unwrap();
        assert_eq!(compressed.decompress_lz4(data.len()).unwrap(), data);
        assert_eq!(
            compressed
                .decompress_lz4(data.len() - 1)
                .unwrap_err()
                .to_string(),
            "invalid data: decompressed payload exceeds maximum size (2 bytes)"
        );
    }

    #[test]
    fn lz4_preserves_malformed_block_error() {
        let invalid = vec![1, 0, 0, 0, 0];
        let expected = decompress_lz4(&invalid, 1024).unwrap_err();
        for error in [
            invalid.decompress_lz4(1024).unwrap_err(),
            invalid.as_slice().decompress_lz4(1024).unwrap_err(),
        ] {
            assert_eq!(error.to_string(), expected.to_string());
            assert!(matches!(error, IoError::InvalidData { .. }));
        }
    }
}

use crate::compression::{CompressionResult, compress_lz4, decompress_lz4};
use crate::{IoError, IoResult, MemoryReader, Serializable, serializable::helper};

/// LZ4/serialization helpers for byte slices.
pub trait ByteLz4Extensions {
    /// Compresses the byte slice using LZ4 with the original length prepended.
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>>;

    /// Decompresses an LZ4 payload that was produced by [`compress_lz4`].
    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>>;

    /// Deserialises the slice into a [`Serializable`] type starting at `offset`.
    fn as_serializable<T: Serializable>(&self, offset: usize) -> IoResult<T>;

    /// Deserialises the slice into a vector of [`Serializable`] values.
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>>;
}

impl ByteLz4Extensions for [u8] {
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>> {
        compress_lz4(self)
    }

    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>> {
        decompress_lz4(self, max_output)
    }

    fn as_serializable<T: Serializable>(&self, offset: usize) -> IoResult<T> {
        if offset > self.len() {
            return Err(IoError::invalid_data(
                "Offset exceeds slice length when reading serialisable value",
            ));
        }
        let mut reader = MemoryReader::new(&self[offset..]);
        T::deserialize(&mut reader)
    }

    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        let mut reader = MemoryReader::new(self);
        helper::deserialize_array::<T>(&mut reader, max)
    }
}

impl ByteLz4Extensions for Vec<u8> {
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>> {
        self.as_slice().compress_lz4()
    }

    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>> {
        self.as_slice().decompress_lz4(max_output)
    }

    fn as_serializable<T: Serializable>(&self, offset: usize) -> IoResult<T> {
        self.as_slice().as_serializable(offset)
    }

    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        self.as_slice().as_serializable_array(max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BinaryWriter;

    #[derive(Debug, PartialEq, Eq)]
    struct ByteValue(u8);

    impl Serializable for ByteValue {
        fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
            reader.read_byte().map(Self)
        }

        fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
            writer.write_bytes(&[self.0])
        }

        fn size(&self) -> usize {
            1
        }
    }

    #[derive(Debug)]
    struct RejectIo;

    impl Serializable for RejectIo {
        fn deserialize(_reader: &mut MemoryReader) -> IoResult<Self> {
            Err(IoError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "fixture read failure",
            )))
        }

        fn serialize(&self, _writer: &mut BinaryWriter) -> IoResult<()> {
            Ok(())
        }

        fn size(&self) -> usize {
            0
        }
    }

    #[test]
    fn test_byte_extensions_compression_round_trip() {
        let data = vec![1u8, 2, 3, 4];
        let compressed = data.compress_lz4().unwrap();
        let decompressed = compressed.decompress_lz4(usize::MAX).unwrap();
        assert_eq!(data, decompressed);

        let repetitive = vec![1u8; 255];
        let compressed = repetitive.compress_lz4().unwrap();
        let decompressed = compressed.decompress_lz4(usize::MAX).unwrap();
        assert!(compressed.len() < repetitive.len());
        assert_eq!(repetitive, decompressed);
    }

    #[test]
    fn test_byte_extensions_compression_errors() {
        let data = vec![1u8; 32];
        let compressed = data.compress_lz4().unwrap();

        let too_small = compressed.decompress_lz4(data.len() - 1);
        assert!(matches!(too_small, Err(IoError::InvalidData { .. })));

        let mut corrupted = compressed.clone();
        corrupted[0] = corrupted[0].wrapping_add(1);
        let corrupted_result = corrupted.decompress_lz4(usize::MAX);
        assert!(matches!(corrupted_result, Err(IoError::InvalidData { .. })));
    }

    #[test]
    fn lz4_preserves_wire_bytes_and_empty_input() {
        let literal = b"abc".as_slice();
        assert_eq!(
            literal.compress_lz4().unwrap(),
            vec![3, 0, 0, 0, 0x30, b'a', b'b', b'c']
        );
        for data in [b"".as_slice(), literal, &[0; 512]] {
            let owned = data.to_vec();
            let compressed = data.compress_lz4().unwrap();
            assert_eq!(compressed, compress_lz4(data).unwrap());
            assert_eq!(owned.compress_lz4().unwrap(), compressed);
            assert_eq!(&compressed[..4], &(data.len() as u32).to_le_bytes());
            assert_eq!(compressed.decompress_lz4(data.len()).unwrap(), data);
            assert_eq!(
                compressed.as_slice().decompress_lz4(data.len()).unwrap(),
                data
            );
        }
    }

    #[test]
    fn lz4_preserves_limit_and_malformed_errors() {
        for data in [
            b"".as_slice(),
            &[1],
            &[1, 2],
            &[1, 2, 3],
            &[1, 0, 0, 0, 0],
            &[0, 4, 0, 0, 0],
        ] {
            let expected = decompress_lz4(data, 16).unwrap_err();
            for error in [
                data.decompress_lz4(16).unwrap_err(),
                data.to_vec().decompress_lz4(16).unwrap_err(),
            ] {
                assert_eq!(error.to_string(), expected.to_string());
                assert!(matches!(error, IoError::InvalidData { .. }));
            }
        }
        assert_eq!(
            [0, 4, 0, 0, 0]
                .as_slice()
                .decompress_lz4(16)
                .unwrap_err()
                .to_string(),
            "invalid data: decompressed payload exceeds maximum size (16 bytes)"
        );
    }

    #[test]
    fn serializable_offset_preserves_boundaries_and_trailing_data() {
        let data = vec![0x99, 0x2a, 0x7e];
        assert_eq!(
            data.as_serializable::<ByteValue>(1).unwrap(),
            ByteValue(0x2a)
        );
        assert_eq!(
            data.as_slice().as_serializable::<ByteValue>(1).unwrap(),
            ByteValue(0x2a)
        );
        assert!(matches!(
            data.as_serializable::<ByteValue>(data.len()),
            Err(IoError::Format)
        ));
        assert!(matches!(
            b"".as_slice().as_serializable::<ByteValue>(0),
            Err(IoError::Format)
        ));
        for offset in [data.len() + 1, usize::MAX] {
            for error in [
                data.as_serializable::<ByteValue>(offset).unwrap_err(),
                data.as_slice()
                    .as_serializable::<ByteValue>(offset)
                    .unwrap_err(),
            ] {
                assert_eq!(
                    error.to_string(),
                    "invalid data: Offset exceeds slice length when reading serialisable value"
                );
                match error {
                    IoError::InvalidData { context, value } => {
                        assert_eq!(context, "invalid data");
                        assert_eq!(
                            value,
                            "Offset exceeds slice length when reading serialisable value"
                        );
                    }
                    other => panic!("expected invalid data, got {other:?}"),
                }
            }
        }
    }

    #[test]
    fn serializable_array_preserves_count_limits_and_noncanonical_prefix() {
        let data = vec![2, 10, 20, 99];
        let expected = vec![ByteValue(10), ByteValue(20)];
        assert_eq!(
            data.as_serializable_array::<ByteValue>(2).unwrap(),
            expected
        );
        assert_eq!(
            data.as_slice()
                .as_serializable_array::<ByteValue>(2)
                .unwrap(),
            expected
        );
        assert!(matches!(
            data.as_serializable_array::<ByteValue>(1),
            Err(IoError::Format)
        ));
        assert!(
            [0, 99]
                .as_slice()
                .as_serializable_array::<ByteValue>(0)
                .unwrap()
                .is_empty()
        );
        for data in [b"".as_slice(), &[2, 10], &[0xfd, 1]] {
            assert!(matches!(
                data.as_serializable_array::<ByteValue>(2),
                Err(IoError::Format)
            ));
        }
        assert_eq!(
            [0xfd, 1, 0, 7]
                .as_slice()
                .as_serializable_array::<ByteValue>(1)
                .unwrap(),
            vec![ByteValue(7)]
        );
    }

    #[test]
    fn serializable_forwards_io_error_without_reclassification() {
        let data = vec![1];
        for error in [
            data.as_serializable::<RejectIo>(0).unwrap_err(),
            data.as_slice().as_serializable::<RejectIo>(0).unwrap_err(),
            data.as_serializable_array::<RejectIo>(1).unwrap_err(),
            data.as_slice()
                .as_serializable_array::<RejectIo>(1)
                .unwrap_err(),
        ] {
            match error {
                IoError::Io(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
                    assert_eq!(error.to_string(), "fixture read failure");
                }
                other => panic!("expected original I/O error, got {other:?}"),
            }
        }
    }
}

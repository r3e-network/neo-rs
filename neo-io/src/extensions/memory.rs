use crate::extensions::byte::ByteLz4Extensions;
use crate::{IoError, IoResult, Serializable, serializable::helper};

/// Extension helpers for read-only byte slices matching
/// `Neo.Extensions.MemoryExtensions`.
pub trait ReadOnlyMemoryExtensions {
    /// Deserialises the span into a vector of [`Serializable`] values.
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>>;

    /// Deserialises the span into a [`Serializable`] value.
    fn as_serializable<T: Serializable>(&self) -> IoResult<T>;

    /// Gets the size in bytes when encoded using Neo's variable length rules.
    fn get_var_size(&self) -> IoResult<usize>;
}

impl ReadOnlyMemoryExtensions for [u8] {
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        <[u8] as ByteLz4Extensions>::as_serializable_array(self, max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        if self.is_empty() {
            return Err(IoError::invalid_data(
                "Cannot deserialize from an empty ReadOnlyMemory",
            ));
        }
        <[u8] as ByteLz4Extensions>::as_serializable(self, 0)
    }

    fn get_var_size(&self) -> IoResult<usize> {
        let length = self.len();
        Ok(helper::get_var_size_usize(length) + length)
    }
}

impl ReadOnlyMemoryExtensions for Vec<u8> {
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>> {
        <[u8] as ReadOnlyMemoryExtensions>::as_serializable_array(self.as_slice(), max)
    }

    fn as_serializable<T: Serializable>(&self) -> IoResult<T> {
        <[u8] as ReadOnlyMemoryExtensions>::as_serializable(self.as_slice())
    }

    fn get_var_size(&self) -> IoResult<usize> {
        self.as_slice().get_var_size()
    }
}

#[cfg(test)]
mod tests {
    use super::ReadOnlyMemoryExtensions;
    use crate::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

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

    #[test]
    fn serializable_preserves_empty_memory_error() {
        let data = Vec::<u8>::new();
        for error in [
            data.as_serializable::<ByteValue>().unwrap_err(),
            data.as_slice().as_serializable::<ByteValue>().unwrap_err(),
        ] {
            assert_eq!(
                error.to_string(),
                "invalid data: Cannot deserialize from an empty ReadOnlyMemory"
            );
            match error {
                IoError::InvalidData { context, value } => {
                    assert_eq!(context, "invalid data");
                    assert_eq!(value, "Cannot deserialize from an empty ReadOnlyMemory");
                }
                other => panic!("expected empty-memory invalid data, got {other:?}"),
            }
        }
    }

    #[test]
    fn serializable_reads_from_start_without_rejecting_trailing_data() {
        let data = vec![42, 99];
        assert_eq!(data.as_serializable::<ByteValue>().unwrap(), ByteValue(42));
        assert_eq!(
            data.as_slice().as_serializable::<ByteValue>().unwrap(),
            ByteValue(42)
        );
    }

    #[test]
    fn serializable_array_preserves_empty_count_range_and_truncation() {
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
    fn var_size_preserves_neo_prefix_boundaries() {
        for (length, prefix_size) in [
            (0, 1),
            (1, 1),
            (0xfc, 1),
            (0xfd, 3),
            (0xffff, 3),
            (0x10000, 5),
        ] {
            let data = vec![0; length];
            assert_eq!(data.get_var_size().unwrap(), length + prefix_size);
            assert_eq!(
                data.as_slice().get_var_size().unwrap(),
                length + prefix_size
            );
        }
    }
}

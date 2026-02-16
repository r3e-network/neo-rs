use crate::compression::{CompressionResult, compress_lz4, decompress_lz4};
use crate::io::{IoError, IoResult, MemoryReader, Serializable, serializable::helper};

/// LZ4/serialization helpers for byte slices used by the core crate.
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

use crate::compression::{compress_lz4, decompress_lz4, CompressionResult};
use crate::io::{IoError, IoResult, MemoryReader, Serializable};

/// Extension helpers for byte slices matching `Neo.Extensions.ByteExtensions`.
pub trait ByteExtensions {
    /// Compresses the byte slice using LZ4 with the original length prepended.
    fn compress_lz4(&self) -> CompressionResult<Vec<u8>>;

    /// Decompresses an LZ4 payload that was produced by [`compress_lz4`].
    fn decompress_lz4(&self, max_output: usize) -> CompressionResult<Vec<u8>>;

    /// Deserialises the slice into a [`Serializable`] type starting at `offset`.
    fn as_serializable<T: Serializable>(&self, offset: usize) -> IoResult<T>;

    /// Deserialises the slice into a vector of [`Serializable`] values.
    fn as_serializable_array<T: Serializable>(&self, max: usize) -> IoResult<Vec<T>>;
}

impl ByteExtensions for [u8] {
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
        let count = reader.read_var_uint()? as usize;
        if count > max {
            return Err(IoError::invalid_data(format!(
                "Serialisable array length ({count}) exceeds maximum ({max})"
            )));
        }

        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(T::deserialize(&mut reader)?);
        }
        Ok(values)
    }
}

impl ByteExtensions for Vec<u8> {
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

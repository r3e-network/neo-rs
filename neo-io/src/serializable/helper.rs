//! Helper utilities mirroring C# Neo serialization helpers.

use super::Serializable;
use crate::{BinaryWriter, IoError, IoResult, MemoryReader, var_int};

/// Helper utilities mirroring C# Neo serialization helpers.
pub struct SerializeHelper;

impl SerializeHelper {
    /// Returns the number of bytes required to encode `value` using Neo variable-length encoding.
    #[inline]
    #[must_use]
    pub const fn get_var_size(value: u64) -> usize {
        var_int::VarInt::encoded_len(value)
    }

    /// Convenience wrapper for `usize` inputs.
    #[inline]
    #[must_use]
    pub fn get_var_size_usize(value: usize) -> usize {
        Self::get_var_size(value as u64)
    }

    /// Returns the size contribution for a byte slice encoded with `write_var_bytes`.
    #[inline]
    #[must_use]
    pub fn get_var_size_bytes(bytes: &[u8]) -> usize {
        Self::get_var_size_usize(bytes.len()) + bytes.len()
    }

    /// Returns the size contribution for a UTF-8 string encoded with `write_var_string`.
    #[inline]
    #[must_use]
    pub fn get_var_size_str(value: &str) -> usize {
        Self::get_var_size_bytes(value.as_bytes())
    }

    /// Returns the encoded size of an array of serializable items.
    #[inline]
    pub fn get_var_size_serializable_slice<T: Serializable>(values: &[T]) -> usize {
        Self::get_var_size_usize(values.len()) + values.iter().map(Serializable::size).sum::<usize>()
    }

    /// Returns the encoded size of a length-prefixed array whose items use custom sizing.
    #[inline]
    pub fn get_var_size_for_slice<T, F>(values: &[T], mut item_size: F) -> usize
    where
        F: FnMut(&T) -> usize,
    {
        Self::get_var_size_usize(values.len()) + values.iter().map(&mut item_size).sum::<usize>()
    }

    /// Serializes an array of `Serializable` items with a Neo length prefix.
    pub fn serialize_array<T: Serializable>(values: &[T], writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_var_int(values.len() as u64)?;
        for value in values {
            value.serialize(writer)?;
        }
        Ok(())
    }

    /// Serializes an array with a Neo length prefix and custom item writer.
    pub fn serialize_array_with<T, F>(
        values: &[T],
        writer: &mut BinaryWriter,
        mut write_item: F,
    ) -> IoResult<()>
    where
        F: FnMut(&T, &mut BinaryWriter) -> IoResult<()>,
    {
        writer.write_var_int(values.len() as u64)?;
        for value in values {
            write_item(value, writer)?;
        }
        Ok(())
    }

    /// Deserializes an array of `Serializable` items with an upper bound check.
    pub fn deserialize_array<T: Serializable>(
        reader: &mut MemoryReader,
        max: usize,
    ) -> IoResult<Vec<T>> {
        let count = reader.read_var_int(max as u64)? as usize;
        if count > max {
            return Err(IoError::invalid_data(format!(
                "array length {count} exceeds maximum {max}"
            )));
        }

        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(T::deserialize(reader)?);
        }
        Ok(result)
    }

    /// Deserializes an array with an upper bound check and custom item reader.
    pub fn deserialize_array_with<T, F>(
        reader: &mut MemoryReader,
        max: usize,
        mut read_item: F,
    ) -> IoResult<Vec<T>>
    where
        F: FnMut(&mut MemoryReader) -> IoResult<T>,
    {
        let count = reader.read_var_int(max as u64)? as usize;
        if count > max {
            return Err(IoError::invalid_data(format!(
                "array length {count} exceeds maximum {max}"
            )));
        }

        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(read_item(reader)?);
        }
        Ok(result)
    }

    /// Deserializes an array whose length must exactly match `expected`.
    ///
    /// This is useful for Neo wire formats where one vector's length is defined by
    /// an earlier field, for example transaction witnesses matching signer count.
    pub fn deserialize_exact_array<T: Serializable>(
        reader: &mut MemoryReader,
        expected: usize,
        mismatch_message: &'static str,
    ) -> IoResult<Vec<T>> {
        let count = reader.read_var_int(expected as u64)? as usize;
        if count != expected {
            return Err(IoError::invalid_data(mismatch_message));
        }

        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            result.push(T::deserialize(reader)?);
        }
        Ok(result)
    }
}

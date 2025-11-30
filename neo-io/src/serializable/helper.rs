//! Helper utilities mirroring C# Neo serialization helpers.

use super::Serializable;
use crate::{BinaryWriter, IoError, IoResult, MemoryReader};

/// Returns the number of bytes required to encode `value` using Neo variable-length encoding.
#[inline]
pub fn get_var_size(value: u64) -> usize {
    if value < 0xFD {
        1
    } else if value <= 0xFFFF {
        1 + 2
    } else if value <= 0xFFFF_FFFF {
        1 + 4
    } else {
        1 + 8
    }
}

/// Convenience wrapper for `usize` inputs.
#[inline]
pub fn get_var_size_usize(value: usize) -> usize {
    get_var_size(value as u64)
}

/// Returns the size contribution for a byte slice encoded with `write_var_bytes`.
#[inline]
pub fn get_var_size_bytes(bytes: &[u8]) -> usize {
    get_var_size_usize(bytes.len()) + bytes.len()
}

/// Returns the size contribution for a UTF-8 string encoded with `write_var_string`.
#[inline]
pub fn get_var_size_str(value: &str) -> usize {
    get_var_size_bytes(value.as_bytes())
}

/// Returns the encoded size of an array of serializable items.
#[inline]
pub fn get_var_size_serializable_slice<T: Serializable>(values: &[T]) -> usize {
    get_var_size_usize(values.len()) + values.iter().map(Serializable::size).sum::<usize>()
}

/// Serializes an array of `Serializable` items with a Neo length prefix.
pub fn serialize_array<T: Serializable>(values: &[T], writer: &mut BinaryWriter) -> IoResult<()> {
    writer.write_var_int(values.len() as u64)?;
    for value in values {
        value.serialize(writer)?;
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

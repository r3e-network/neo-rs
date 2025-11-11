use alloc::vec::Vec;

use neo_base::encoding::{write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

pub fn read_array<R, T>(reader: &mut R) -> Result<Vec<T>, DecodeError>
where
    R: NeoRead,
    T: NeoDecode,
{
    let len = reader.read_varint()? as usize;
    let mut values = Vec::with_capacity(len);
    for _ in 0..len {
        values.push(T::neo_decode(reader)?);
    }
    Ok(values)
}

pub fn write_array<W, T>(writer: &mut W, values: &[T])
where
    W: NeoWrite,
    T: NeoEncode,
{
    write_varint(writer, values.len() as u64);
    for value in values {
        value.neo_encode(writer);
    }
}

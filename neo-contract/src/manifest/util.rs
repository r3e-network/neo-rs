use alloc::vec::Vec;

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    write_varint,
};

/// Writes a Neo varint length followed by each encoded element.
pub(crate) fn encode_vec<W, T>(writer: &mut W, items: &[T])
where
    W: NeoWrite,
    T: NeoEncode,
{
    write_varint(writer, items.len() as u64);
    for item in items {
        item.neo_encode(writer);
    }
}

/// Reads a Neo varint length and decodes that many elements.
pub(crate) fn decode_vec<R, T>(reader: &mut R) -> Result<Vec<T>, DecodeError>
where
    R: NeoRead,
    T: NeoDecode,
{
    let len = reader.read_varint()?;
    if len > usize::MAX as u64 {
        return Err(DecodeError::LengthOutOfRange {
            len,
            max: usize::MAX as u64,
        });
    }

    let mut items = Vec::with_capacity(len as usize);
    for _ in 0..len {
        items.push(T::neo_decode(reader)?);
    }
    Ok(items)
}

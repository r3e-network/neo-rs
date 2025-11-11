use neo_base::encoding::{DecodeError, NeoRead, NeoWrite};

pub(super) fn write_i16<W: NeoWrite>(writer: &mut W, value: i16) {
    writer.write_bytes(&value.to_le_bytes());
}

pub(super) fn read_i16<R: NeoRead>(reader: &mut R) -> Result<i16, DecodeError> {
    let mut buf = [0u8; 2];
    reader.read_into(&mut buf)?;
    Ok(i16::from_le_bytes(buf))
}

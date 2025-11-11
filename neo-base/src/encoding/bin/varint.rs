use super::{DecodeError, NeoRead, NeoWrite};

#[inline]
pub fn write_varint<W: NeoWrite + ?Sized>(writer: &mut W, value: u64) {
    let mut buf = [0u8; 9];
    let (len, data) = to_varint_le(value, &mut buf);
    writer.write_bytes(&data[..len]);
}

#[inline]
pub fn read_varint<R: NeoRead + ?Sized>(reader: &mut R) -> Result<u64, DecodeError> {
    let tag = reader.read_u8()?;
    match tag {
        value @ 0x00..=0xFC => Ok(value as u64),
        0xFD => {
            let value = reader.read_u16()?;
            if value < 0xFD {
                Err(DecodeError::InvalidVarIntTag(0xFD))
            } else {
                Ok(value as u64)
            }
        }
        0xFE => {
            let value = reader.read_u32()?;
            if value < 0x0001_0000 {
                Err(DecodeError::InvalidVarIntTag(0xFE))
            } else {
                Ok(value as u64)
            }
        }
        0xFF => {
            let value = reader.read_u64()?;
            if value < 0x0000_0001_0000_0000 {
                Err(DecodeError::InvalidVarIntTag(0xFF))
            } else {
                Ok(value)
            }
        }
    }
}

#[inline]
pub fn to_varint_le(value: u64, scratch: &mut [u8; 9]) -> (usize, [u8; 9]) {
    scratch.fill(0);
    if value < 0xFD {
        scratch[0] = value as u8;
        (1, *scratch)
    } else if value <= 0xFFFF {
        scratch[0] = 0xFD;
        scratch[1..3].copy_from_slice(&(value as u16).to_le_bytes());
        (3, *scratch)
    } else if value <= 0xFFFF_FFFF {
        scratch[0] = 0xFE;
        scratch[1..5].copy_from_slice(&(value as u32).to_le_bytes());
        (5, *scratch)
    } else {
        scratch[0] = 0xFF;
        scratch[1..9].copy_from_slice(&value.to_le_bytes());
        (9, *scratch)
    }
}

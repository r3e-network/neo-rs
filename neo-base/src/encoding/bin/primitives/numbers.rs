use super::super::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

impl NeoEncode for bool {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self as u8);
    }
}

impl NeoDecode for bool {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::InvalidValue("bool")),
        }
    }
}

macro_rules! impl_int {
    ($ty:ty, $write:ident, $read:ident) => {
        impl NeoEncode for $ty {
            #[inline]
            fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
                writer.$write(*self);
            }
        }

        impl NeoDecode for $ty {
            #[inline]
            fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
                reader.$read()
            }
        }
    };
}

impl_int!(u8, write_u8, read_u8);
impl_int!(u16, write_u16, read_u16);
impl_int!(u32, write_u32, read_u32);
impl_int!(u64, write_u64, read_u64);

impl NeoEncode for i64 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.to_le_bytes());
    }
}

impl NeoDecode for i64 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; 8];
        reader.read_into(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }
}

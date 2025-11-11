use alloc::string::String;

use super::super::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

impl NeoEncode for String {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self.as_bytes());
    }
}

impl NeoEncode for str {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self.as_bytes());
    }
}

impl NeoDecode for String {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let buf = reader.read_var_bytes(u32::MAX as u64)?;
        String::from_utf8(buf).map_err(|_| DecodeError::InvalidUtf8)
    }
}

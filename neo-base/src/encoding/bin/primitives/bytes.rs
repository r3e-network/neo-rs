use alloc::vec::Vec;

use super::super::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

impl NeoEncode for [u8] {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self);
    }
}

impl NeoEncode for Vec<u8> {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self);
    }
}

impl NeoDecode for Vec<u8> {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_var_bytes(u64::MAX)
    }
}

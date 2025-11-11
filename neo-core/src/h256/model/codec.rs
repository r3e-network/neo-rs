use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::definition::{H256, H256_SIZE};

impl NeoEncode for H256 {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(self.as_le_bytes());
    }
}

impl NeoDecode for H256 {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; H256_SIZE];
        reader.read_into(&mut buf)?;
        Ok(Self::from_le_bytes(buf))
    }
}

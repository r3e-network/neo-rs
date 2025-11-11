use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::WitnessScopes;

impl NeoEncode for WitnessScopes {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.scopes);
    }
}

impl NeoDecode for WitnessScopes {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            scopes: reader.read_u8()?,
        })
    }
}

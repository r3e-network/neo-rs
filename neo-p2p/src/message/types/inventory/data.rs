use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash256,
    Bytes,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadWithData {
    pub hash: Hash256,
    pub data: Bytes,
}

impl PayloadWithData {
    pub fn new(hash: Hash256, data: Bytes) -> Self {
        Self { hash, data }
    }
}

impl NeoEncode for PayloadWithData {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.hash.neo_encode(writer);
        self.data.neo_encode(writer);
    }
}

impl NeoDecode for PayloadWithData {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            hash: Hash256::neo_decode(reader)?,
            data: Bytes::neo_decode(reader)?,
        })
    }
}

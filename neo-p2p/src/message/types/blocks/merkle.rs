use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    Bytes,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleBlockPayload {
    pub block: Bytes,
}

impl NeoEncode for MerkleBlockPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.block.neo_encode(writer);
    }
}

impl NeoDecode for MerkleBlockPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            block: Bytes::neo_decode(reader)?,
        })
    }
}

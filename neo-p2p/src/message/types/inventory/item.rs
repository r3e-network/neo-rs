use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash256,
};

use super::InventoryKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    pub kind: InventoryKind,
    pub hash: Hash256,
}

impl NeoEncode for InventoryItem {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.kind.neo_encode(writer);
        self.hash.neo_encode(writer);
    }
}

impl NeoDecode for InventoryItem {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            kind: InventoryKind::neo_decode(reader)?,
            hash: Hash256::neo_decode(reader)?,
        })
    }
}

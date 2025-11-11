use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    hash::Hash256,
};

use super::util::{read_i16, write_i16};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetBlocksPayload {
    pub hash_start: Hash256,
    pub count: i16,
}

impl GetBlocksPayload {
    fn validate(count: i16) -> Result<(), DecodeError> {
        if count == -1 || count > 0 {
            Ok(())
        } else {
            Err(DecodeError::InvalidValue("getblocks count"))
        }
    }
}

impl NeoEncode for GetBlocksPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        debug_assert!(
            Self::validate(self.count).is_ok(),
            "invalid getblocks count"
        );
        self.hash_start.neo_encode(writer);
        write_i16(writer, self.count);
    }
}

impl NeoDecode for GetBlocksPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let hash_start = Hash256::neo_decode(reader)?;
        let count = read_i16(reader)?;
        Self::validate(count)?;
        Ok(Self { hash_start, count })
    }
}

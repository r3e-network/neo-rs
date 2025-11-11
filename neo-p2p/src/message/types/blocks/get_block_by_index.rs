use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::super::MAX_HEADERS_COUNT;
use super::util::{read_i16, write_i16};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetBlockByIndexPayload {
    pub start_index: u32,
    pub count: i16,
}

impl GetBlockByIndexPayload {
    fn validate(count: i16) -> Result<(), DecodeError> {
        if count == -1 || (count > 0 && count as u64 <= MAX_HEADERS_COUNT) {
            Ok(())
        } else {
            Err(DecodeError::InvalidValue("getblockbyindex count"))
        }
    }
}

impl NeoEncode for GetBlockByIndexPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        debug_assert!(
            Self::validate(self.count).is_ok(),
            "invalid getblockbyindex count"
        );
        self.start_index.neo_encode(writer);
        write_i16(writer, self.count);
    }
}

impl NeoDecode for GetBlockByIndexPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let start_index = u32::neo_decode(reader)?;
        let count = read_i16(reader)?;
        Self::validate(count)?;
        Ok(Self { start_index, count })
    }
}

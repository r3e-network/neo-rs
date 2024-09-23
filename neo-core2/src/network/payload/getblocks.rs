use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter};
use crate::util::Uint256;

// Maximum inventory hashes number is limited to 500.
const MAX_HASHES_COUNT: usize = 500;

// GetBlocks contains getblocks message payload fields.
pub struct GetBlocks {
    // Hash of the latest block that node requests.
    pub hash_start: Uint256,
    pub count: i16,
}

impl GetBlocks {
    // NewGetBlocks returns a new GetBlocks object.
    pub fn new(hash_start: Uint256, count: i16) -> Self {
        Self { hash_start, count }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.hash_start.decode_binary(br)?;
        self.count = br.read_u16_le()? as i16;
        if self.count < -1 || self.count == 0 {
            return Err(Box::new(DecodeError::InvalidCount));
        }
        Ok(())
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        self.hash_start.encode_binary(bw)?;
        bw.write_u16_le(self.count as u16)?;
        Ok(())
    }
}

#[derive(Debug)]
enum DecodeError {
    InvalidCount,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid count")
    }
}

impl Error for DecodeError {}

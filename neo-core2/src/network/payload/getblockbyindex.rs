use std::fmt;

use crate::io::{BinReader, BinWriter, Serializable};

// GetBlockByIndex payload.
pub struct GetBlockByIndex {
    pub index_start: u32,
    pub count: i16,
}

impl GetBlockByIndex {
    // NewGetBlockByIndex returns GetBlockByIndex payload with the specified start index and count.
    pub fn new(index_start: u32, count: i16) -> Self {
        GetBlockByIndex {
            index_start,
            count,
        }
    }
}

impl Serializable for GetBlockByIndex {
    // DecodeBinary implements the Serializable interface.
    fn decode_binary(&mut self, br: &mut BinReader) {
        self.index_start = br.read_u32_le();
        self.count = br.read_u16_le() as i16;
        if self.count < -1 || self.count == 0 || self.count > MaxHeadersAllowed {
            br.err = Some(fmt::format(format_args!("invalid block count: {}", self.count)));
        }
    }

    // EncodeBinary implements the Serializable interface.
    fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_u32_le(self.index_start);
        bw.write_u16_le(self.count as u16);
    }
}

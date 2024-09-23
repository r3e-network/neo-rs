use std::time::{SystemTime, UNIX_EPOCH};
use crate::io::{BinReader, BinWriter};

// Ping payload for ping/pong payloads.
pub struct Ping {
    // Index of the last block.
    pub last_block_index: u32,
    // Timestamp.
    pub timestamp: u32,
    // Nonce of the server.
    pub nonce: u32,
}

impl Ping {
    // NewPing creates new Ping payload.
    pub fn new(block_index: u32, nonce: u32) -> Self {
        Ping {
            last_block_index: block_index,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as u32,
            nonce,
        }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.last_block_index = br.read_u32_le();
        self.timestamp = br.read_u32_le();
        self.nonce = br.read_u32_le();
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_u32_le(self.last_block_index);
        bw.write_u32_le(self.timestamp);
        bw.write_u32_le(self.nonce);
    }
}

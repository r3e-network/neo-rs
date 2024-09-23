use crate::io::{BinReader, BinWriter};
use crate::util::Uint256;

// MaxMPTHashesCount is the maximum number of the requested MPT nodes hashes.
const MAX_MPT_HASHES_COUNT: usize = 32;

// MPTInventory payload.
pub struct MPTInventory {
    // A list of the requested MPT nodes hashes.
    hashes: Vec<Uint256>,
}

impl MPTInventory {
    // NewMPTInventory returns an MPTInventory.
    pub fn new(hashes: Vec<Uint256>) -> Self {
        MPTInventory { hashes }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.hashes = br.read_array(MAX_MPT_HASHES_COUNT);
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_array(&self.hashes);
    }
}

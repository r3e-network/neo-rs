use std::error::Error;
use std::convert::TryInto;

use crate::core::block::Header;
use crate::io::{BinReader, BinWriter};
use crate::util::Uint256;

// MerkleBlock represents a merkle block packet payload.
pub struct MerkleBlock {
    header: Header,
    tx_count: usize,
    hashes: Vec<Uint256>,
    flags: Vec<u8>,
}

impl MerkleBlock {
    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.header = Header::default();
        self.header.decode_binary(br)?;

        let tx_count = br.read_var_uint()? as usize;
        if tx_count > Header::MAX_TRANSACTIONS_PER_BLOCK {
            return Err(Box::new(Header::ERR_MAX_CONTENTS_PER_BLOCK));
        }
        self.tx_count = tx_count;
        self.hashes = br.read_array(self.tx_count)?;
        if tx_count != self.hashes.len() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid tx count")));
        }
        self.flags = br.read_var_bytes((tx_count + 7) / 8)?;
        Ok(())
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        self.header.encode_binary(bw)?;

        bw.write_var_uint(self.tx_count.try_into()?)?;
        bw.write_array(&self.hashes)?;
        bw.write_var_bytes(&self.flags)?;
        Ok(())
    }
}

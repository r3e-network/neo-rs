use alloc::vec::Vec;

use crate::{
    h256::H256,
    io::{read_array, write_array},
};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::super::types::Header;

#[derive(Debug, Clone)]
pub struct TrimmedBlock {
    pub header: Header,
    pub hashes: Vec<H256>,
}

impl TrimmedBlock {
    pub fn new(header: Header, hashes: Vec<H256>) -> Self {
        Self { header, hashes }
    }
}

impl NeoEncode for TrimmedBlock {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.header.neo_encode(writer);
        write_array(writer, &self.hashes);
    }
}

impl NeoDecode for TrimmedBlock {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let header = Header::neo_decode(reader)?;
        let hashes = read_array(reader)?;
        Ok(Self { header, hashes })
    }
}

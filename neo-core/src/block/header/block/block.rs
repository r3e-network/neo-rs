use alloc::vec::Vec;

use crate::{h256::H256, io::write_array, tx::Tx};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::super::types::Header;
use crate::block::merkle::compute_merkle_root;

#[derive(Debug, Clone)]
pub struct Block {
    pub header: Header,
    pub txs: Vec<Tx>,
}

impl Block {
    pub fn new(header: Header, txs: Vec<Tx>) -> Self {
        Self { header, txs }
    }

    pub fn trimmed(&self) -> super::TrimmedBlock {
        super::TrimmedBlock::new(
            self.header.clone(),
            self.txs.iter().map(|tx| tx.hash()).collect(),
        )
    }

    pub fn recompute_merkle_root(&mut self) {
        let hashes: Vec<H256> = self.txs.iter().map(|tx| tx.hash()).collect();
        self.header.merkle_root = compute_merkle_root(&hashes);
        self.header.hash = None;
    }
}

impl NeoEncode for Block {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.header.neo_encode(writer);
        write_array(writer, &self.txs);
    }
}

impl NeoDecode for Block {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let header = Header::neo_decode(reader)?;
        let txs: Vec<Tx> = crate::io::read_array(reader)?;
        if txs.is_empty() {
            if header.merkle_root != H256::default() {
                return Err(DecodeError::InvalidValue("MerkleRoot"));
            }
        } else {
            let hashes: Vec<H256> = txs.iter().map(|tx| tx.hash()).collect();
            if compute_merkle_root(&hashes) != header.merkle_root {
                return Err(DecodeError::InvalidValue("MerkleRoot"));
            }
        }
        Ok(Self { header, txs })
    }
}

use crate::{UInt256, Witness, network::p2p::payloads::Transaction};
use serde::{Deserialize, Serialize};

use super::block_header::BlockHeader;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }

    /// Returns the block hash (delegates to the header).
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }

    /// Returns the block index (height).
    pub fn index(&self) -> u32 {
        self.header.index()
    }

    pub fn primary_witness(&self) -> Option<&Witness> {
        self.header.witnesses.first()
    }
}

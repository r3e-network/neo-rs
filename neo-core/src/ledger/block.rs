use crate::{UInt256, Witness, network::p2p::payloads::Transaction};
use serde::{Deserialize, Serialize};

use super::block_header::BlockHeader;

/// A Neo blockchain block: a header plus the transactions it contains.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Block {
    /// The block header carrying height, timestamp, Merkle root, and consensus metadata.
    pub header: BlockHeader,
    /// The transactions included in this block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Creates a block from a header and its transaction list.
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

    /// Returns the witness signing the header, if present.
    pub fn primary_witness(&self) -> Option<&Witness> {
        self.header.witnesses.first()
    }
}

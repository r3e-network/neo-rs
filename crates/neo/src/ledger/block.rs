use crate::{network::p2p::payloads::Transaction, Witness};
use serde::{Deserialize, Serialize};

use super::block_header::BlockHeader;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    pub fn primary_witness(&self) -> Option<&Witness> {
        self.header.witnesses.first()
    }
}

impl Default for Block {
    fn default() -> Self {
        Self {
            header: BlockHeader::default(),
            transactions: Vec::new(),
        }
    }
}

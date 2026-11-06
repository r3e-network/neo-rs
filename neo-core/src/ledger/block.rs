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

    /// Converts this ledger block into the single-witness payload block that the
    /// application engine and native contracts consume.
    ///
    /// The ledger representation keeps a witness *list*; the payload representation
    /// carries exactly one header witness. Only the first witness is projected.
    pub fn to_payload_block(&self) -> neo_protocol::payloads::block::Block {
        let mut header = neo_protocol::payloads::Header::new();
        header.set_version(self.header.version);
        header.set_prev_hash(self.header.previous_hash);
        header.set_merkle_root(self.header.merkle_root);
        header.set_timestamp(self.header.timestamp);
        header.set_nonce(self.header.nonce);
        header.set_index(self.header.index);
        header.set_primary_index(self.header.primary_index);
        header.set_next_consensus(self.header.next_consensus);
        if let Some(witness) = self.header.witnesses.first() {
            header.witness = witness.clone();
        }
        neo_protocol::payloads::block::Block {
            header,
            transactions: self.transactions.clone(),
        }
    }
}

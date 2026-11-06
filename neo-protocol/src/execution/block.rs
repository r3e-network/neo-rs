use super::BlockHeader;
use crate::payloads::Transaction;
use crate::witness::Witness;
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Execution block representation, distinct from the network payload block.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Block {
    /// Execution header carrying a witness list.
    pub header: BlockHeader,
    /// Included transactions.
    pub transactions: Vec<Transaction>,
}
impl Block {
    /// Creates a block from an execution header and transaction list.
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }
    /// Gets the header hash.
    pub fn hash(&self) -> UInt256 {
        self.header.hash()
    }
    /// Gets the block height.
    pub fn index(&self) -> u32 {
        self.header.index()
    }
    /// Gets the first header witness, if present.
    pub fn primary_witness(&self) -> Option<&Witness> {
        self.header.witnesses.first()
    }
    /// Projects this execution block into the single-witness network payload block
    /// that the application engine and native contracts consume.
    ///
    /// The execution representation keeps a witness *list*; the payload representation
    /// carries exactly one header witness. Only the first witness is projected.
    pub fn to_payload_block(&self) -> crate::payloads::Block {
        super::conversions::convert_ledger_block(self.clone())
    }
}

/// Projects a network payload block into its execution representation.
///
/// The application engine persists [`crate::payloads::Block`] (single-witness
/// header with private fields), while the ledger native contract and
/// `TrimmedBlock` operate on the execution [`Block`] (witness-list header with
/// public fields). This conversion bridges the two representations without
/// changing block semantics.
impl From<&crate::payloads::Block> for Block {
    fn from(block: &crate::payloads::Block) -> Self {
        let header = &block.header;
        let execution_header = BlockHeader::new(
            header.version(),
            *header.prev_hash(),
            *header.merkle_root(),
            header.timestamp(),
            header.nonce(),
            header.index(),
            header.primary_index(),
            *header.next_consensus(),
            vec![header.witness.clone()],
        );
        Block::new(execution_header, block.transactions.clone())
    }
}

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
    /// Gets the header hash with potential wire errors.
    pub fn try_hash(&self) -> neo_io::IoResult<UInt256> {
        self.header.try_hash()
    }
    /// Gets the block height.
    pub fn index(&self) -> u32 {
        self.header.index()
    }
    /// Gets the previous block hash.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.header.previous_hash
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

    /// Returns the serialized size of the block.
    /// This is an approximation based on header size plus transaction sizes.
    pub fn size(&self) -> usize {
        use neo_io::serializable::helper::get_var_size_usize;
        use neo_io::Serializable;
        // Header size + transaction count varint + sum of transaction sizes
        let header_size = self.header.size();
        let tx_count_size = get_var_size_usize(self.transactions.len());
        let tx_total_size: usize = self.transactions.iter().map(|tx| Serializable::size(tx)).sum();
        header_size + tx_count_size + tx_total_size
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

use super::block_data::BlockData;
use crate::messages::ConsensusPayload;
use neo_primitives::UInt256;

/// Events emitted by the consensus service
#[derive(Debug, Clone)]
pub enum ConsensusEvent {
    /// Block has been committed with complete data for assembly
    BlockCommitted {
        /// Index of the committed block.
        block_index: u32,
        /// Hash of the committed block.
        block_hash: UInt256,
        /// Complete block data for upper layer to assemble the final Block structure
        block_data: BlockData,
    },
    /// View has changed
    ViewChanged {
        /// Index of the block being processed.
        block_index: u32,
        /// Previous view number.
        old_view: u8,
        /// New view number.
        new_view: u8,
    },
    /// Need to broadcast a message
    BroadcastMessage(ConsensusPayload),
    /// Request transactions from mempool
    RequestTransactions {
        /// Index of the block being built.
        block_index: u32,
        /// Maximum number of transactions to request.
        max_count: usize,
        /// Transaction hashes more than `F` validators reported invalid this
        /// round; the primary must exclude these from the proposal (C# v3.10.1
        /// `EnsureMaxBlockLimitation` `InvalidTransactions` F-skip).
        invalid_tx_hashes: Vec<UInt256>,
    },
    /// Request the exact transactions referenced by a primary proposal.
    RequestProposalTransactions {
        /// Index of the block being validated.
        block_index: u32,
        /// Proposed transaction hashes to resolve locally.
        transaction_hashes: Vec<UInt256>,
    },
}

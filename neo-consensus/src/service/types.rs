use crate::messages::ConsensusPayload;
use neo_primitives::UInt256;

/// Block data for assembly by upper layers
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block index
    pub block_index: u32,
    /// Block timestamp
    pub timestamp: u64,
    /// Block nonce
    pub nonce: u64,
    /// Primary validator index
    pub primary_index: u8,
    /// Transaction hashes included in the block
    pub transaction_hashes: Vec<UInt256>,
    /// Commit signatures from validators (`validator_index`, signature)
    pub signatures: Vec<(u8, Vec<u8>)>,
    /// Validator public keys for multi-sig witness construction
    pub validator_pubkeys: Vec<neo_crypto::ECPoint>,
    /// Required signature count (M in M-of-N multi-sig)
    pub required_signatures: usize,
}

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
    },
}

/// Commands that can be sent to the consensus service
#[derive(Debug)]
pub enum ConsensusCommand {
    /// Start consensus for a new block
    Start {
        /// Index of the block to start consensus for.
        block_index: u32,
        /// Timestamp for the new block.
        timestamp: u64,
    },
    /// Process a received consensus message
    ProcessMessage(ConsensusPayload),
    /// Timer tick (for timeout handling)
    TimerTick {
        /// Current timestamp.
        timestamp: u64,
    },
    /// Transactions received from mempool
    TransactionsReceived {
        /// Hashes of received transactions.
        tx_hashes: Vec<UInt256>,
    },
    /// Stop the consensus service
    Stop,
}

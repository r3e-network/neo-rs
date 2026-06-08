use crate::messages::ConsensusPayload;
use neo_primitives::UInt256;

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

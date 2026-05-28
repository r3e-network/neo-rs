use crate::UInt160;
use crate::UInt256;

// ============ Marker Traits ============

/// Trait for network messages.
///
/// Implementations should provide serialization for network transmission.
pub trait NetworkMessage: Send + Sync {
    /// Returns the command name for this message type.
    fn command(&self) -> &str;

    /// Serializes the message to bytes.
    fn serialize(&self) -> Vec<u8>;
}

/// Trait for block data.
///
/// Provides common operations on blocks without exposing internal structure.
pub trait BlockLike: Send + Sync {
    /// Associated type for transactions in this block.
    type Transaction;

    /// Returns the block hash.
    fn hash(&self) -> UInt256;

    /// Returns the block index (height).
    fn index(&self) -> u32;

    /// Returns the block timestamp.
    fn timestamp(&self) -> u64;

    /// Returns the previous block hash.
    fn prev_hash(&self) -> UInt256;

    /// Returns the merkle root of transactions.
    fn merkle_root(&self) -> UInt256;

    /// Returns the number of transactions.
    fn transaction_count(&self) -> usize;

    /// Returns the serialized size of the block in bytes.
    fn size(&self) -> usize;
}

/// Trait for block header data.
///
/// Headers are blocks without transaction data.
pub trait HeaderLike: Send + Sync {
    /// Returns the header hash.
    fn hash(&self) -> UInt256;

    /// Returns the block index (height).
    fn index(&self) -> u32;

    /// Returns the timestamp.
    fn timestamp(&self) -> u64;

    /// Returns the previous block hash.
    fn prev_hash(&self) -> UInt256;

    /// Returns the merkle root.
    fn merkle_root(&self) -> UInt256;
}

/// Trait for transaction data.
///
/// Provides common operations on transactions.
pub trait TransactionLike: Send + Sync {
    /// Returns the transaction hash.
    fn hash(&self) -> UInt256;

    /// Returns the sender account (first signer).
    fn sender(&self) -> Option<UInt160>;

    /// Returns the system fee.
    fn system_fee(&self) -> i64;

    /// Returns the network fee.
    fn network_fee(&self) -> i64;

    /// Returns the valid until block height.
    fn valid_until_block(&self) -> u32;
}

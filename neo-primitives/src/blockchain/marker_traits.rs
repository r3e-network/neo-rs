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


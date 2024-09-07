/// Represents the status of a transaction in the NEO blockchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainsTransactionType {
    /// The transaction does not exist in the blockchain or memory pool.
    NotExist,
    /// The transaction exists in the memory pool but not yet in the blockchain.
    ExistsInPool,
    /// The transaction exists in the blockchain.
    ExistsInLedger,
}

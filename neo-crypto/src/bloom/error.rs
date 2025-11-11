#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BloomError {
    #[error("bloom filter: bit length must be greater than zero")]
    InvalidBitLength,
    #[error("bloom filter: hash function count must be greater than zero")]
    InvalidHashFunctionCount,
}

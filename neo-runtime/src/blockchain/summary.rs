use neo_base::hash::Hash256;

/// Lightweight representation of block metadata tracked by the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub index: u64,
    pub hash: Hash256,
    pub previous_hash: Hash256,
    pub timestamp_ms: u64,
    pub transaction_count: usize,
    pub size_bytes: u64,
    pub fees_collected: u64,
}

impl BlockSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        index: u64,
        hash: Hash256,
        previous_hash: Hash256,
        timestamp_ms: u64,
        transaction_count: usize,
        size_bytes: u64,
        fees_collected: u64,
    ) -> Self {
        Self {
            index,
            hash,
            previous_hash,
            timestamp_ms,
            transaction_count,
            size_bytes,
            fees_collected,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlockWindowStats {
    pub block_count: usize,
    pub duration_ms: u64,
    pub total_bytes: u64,
    pub total_transactions: u64,
}

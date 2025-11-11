use neo_base::hash::Hash256;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RuntimeStats {
    pub height: u64,
    pub base_fee: u64,
    pub byte_fee: u64,
    pub mempool_size: usize,
    pub total_transactions: u64,
    pub total_fees: u64,
    pub total_size_bytes: u64,
    pub last_block_hash: Option<Hash256>,
    pub last_block_timestamp_ms: Option<u64>,
    pub last_block_size_bytes: u64,
    pub pending_fees: u64,
    pub pending_size_bytes: u64,
    pub avg_block_interval_ms: Option<f64>,
    pub avg_block_size_bytes: Option<f64>,
    pub throughput_bytes_per_sec: Option<f64>,
    pub throughput_tps: Option<f64>,
}

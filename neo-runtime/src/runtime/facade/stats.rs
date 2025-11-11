use crate::runtime::RuntimeStats;

use super::Runtime;

impl Runtime {
    pub fn stats(&self) -> RuntimeStats {
        let last_block = self.blockchain.last_block();
        let window_stats = self.blockchain.window_stats();
        RuntimeStats {
            height: self.blockchain.height(),
            base_fee: self.fees.base_fee(),
            byte_fee: self.fees.byte_fee(),
            mempool_size: self.tx_pool.len(),
            total_transactions: self.blockchain.total_transactions(),
            total_fees: self.blockchain.total_fees(),
            total_size_bytes: self.blockchain.total_size_bytes(),
            last_block_hash: last_block.map(|block| block.hash),
            last_block_timestamp_ms: last_block.map(|block| block.timestamp_ms),
            last_block_size_bytes: last_block.map(|block| block.size_bytes).unwrap_or(0),
            pending_fees: self.tx_pool.total_fees(),
            pending_size_bytes: self.tx_pool.total_size_bytes(),
            avg_block_interval_ms: window_stats.as_ref().map(|stats| {
                stats.duration_ms as f64 / (stats.block_count.saturating_sub(1) as f64)
            }),
            avg_block_size_bytes: window_stats
                .as_ref()
                .map(|stats| stats.total_bytes as f64 / stats.block_count as f64),
            throughput_bytes_per_sec: window_stats
                .as_ref()
                .map(|stats| (stats.total_bytes as f64) / (stats.duration_ms as f64 / 1000.0)),
            throughput_tps: window_stats.as_ref().map(|stats| {
                (stats.total_transactions as f64) / (stats.duration_ms as f64 / 1000.0)
            }),
        }
    }
}

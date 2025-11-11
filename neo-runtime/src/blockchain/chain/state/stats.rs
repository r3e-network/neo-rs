use super::super::BlockWindowStats;
use super::Blockchain;

impl Blockchain {
    pub fn window_stats(&self) -> Option<BlockWindowStats> {
        if self.committed.len() < 2 {
            return None;
        }
        let first = self.committed.front()?;
        let last = self.committed.back()?;
        if last.timestamp_ms <= first.timestamp_ms {
            return None;
        }
        let duration = last.timestamp_ms.saturating_sub(first.timestamp_ms);
        if duration == 0 {
            return None;
        }
        let blocks = self.committed.len();
        let bytes = self.committed.iter().map(|block| block.size_bytes).sum();
        let txs = self
            .committed
            .iter()
            .map(|block| block.transaction_count as u64)
            .sum();
        Some(BlockWindowStats {
            block_count: blocks,
            duration_ms: duration,
            total_bytes: bytes,
            total_transactions: txs,
        })
    }
}

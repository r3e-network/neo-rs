use neo_base::hash::Hash256;

use super::{Blockchain, RECENT_BLOCK_LIMIT};
use crate::blockchain::BlockSummary;

impl Blockchain {
    pub fn apply_block(&mut self, summary: BlockSummary) {
        if summary.index != self.height + 1 {
            return;
        }
        self.height = summary.index;
        self.total_transactions = self
            .total_transactions
            .saturating_add(summary.transaction_count as u64);
        self.total_fees = self.total_fees.saturating_add(summary.fees_collected);
        self.total_bytes = self.total_bytes.saturating_add(summary.size_bytes);
        self.committed.push_back(summary);
        while self.committed.len() > RECENT_BLOCK_LIMIT {
            self.committed.pop_front();
        }
    }

    pub fn rollback_to(&mut self, target_height: u64) {
        if target_height >= self.height {
            return;
        }
        while let Some(last) = self.committed.pop_back() {
            if last.index <= target_height {
                self.committed.push_back(last);
                break;
            }
            self.height -= 1;
            self.total_transactions = self
                .total_transactions
                .saturating_sub(last.transaction_count as u64);
            self.total_fees = self.total_fees.saturating_sub(last.fees_collected);
            self.total_bytes = self.total_bytes.saturating_sub(last.size_bytes);
        }
        self.height = self.committed.back().map(|b| b.index).unwrap_or(0);
    }

    pub fn sync_height(&mut self, target_height: u64) {
        if target_height == self.height {
            return;
        }
        self.committed.clear();
        self.total_transactions = 0;
        self.total_fees = 0;
        self.total_bytes = 0;
        if target_height > 0 {
            self.committed.push_back(BlockSummary {
                index: target_height,
                hash: Hash256::ZERO,
                previous_hash: Hash256::ZERO,
                timestamp_ms: 0,
                transaction_count: 0,
                size_bytes: 0,
                fees_collected: 0,
            });
        }
        self.height = target_height;
    }
}

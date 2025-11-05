use std::collections::VecDeque;

use crate::txpool::PendingTransaction;

/// Lightweight representation of block metadata tracked by the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub index: u64,
    pub transaction_count: usize,
    pub fees_collected: u64,
}

impl BlockSummary {
    pub fn new(index: u64, transaction_count: usize, fees_collected: u64) -> Self {
        Self {
            index,
            transaction_count,
            fees_collected,
        }
    }
}

/// Minimal blockchain state tracker used by the runtime to commit blocks produced
/// by consensus. Persistence lives in higher layers; this component focuses on
/// in-memory bookkeeping and validation glue.
#[derive(Debug, Default)]
pub struct Blockchain {
    height: u64,
    committed: Vec<BlockSummary>,
    pending: VecDeque<PendingTransaction>,
    total_transactions: u64,
    total_fees: u64,
}

impl Blockchain {
    pub fn new() -> Self {
        Self {
            height: 0,
            committed: Vec::new(),
            pending: VecDeque::new(),
            total_transactions: 0,
            total_fees: 0,
        }
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn last_block(&self) -> Option<&BlockSummary> {
        self.committed.last()
    }

    pub fn queue_transaction(&mut self, tx: PendingTransaction) {
        self.pending.push_back(tx);
    }

    pub fn pending_transactions(&self) -> impl Iterator<Item = &PendingTransaction> {
        self.pending.iter()
    }

    pub fn take_pending(&mut self, limit: usize) -> Vec<PendingTransaction> {
        let mut drained = Vec::with_capacity(limit.min(self.pending.len()));
        for _ in 0..limit {
            if let Some(tx) = self.pending.pop_front() {
                drained.push(tx);
            } else {
                break;
            }
        }
        drained
    }

    pub fn apply_block(&mut self, summary: BlockSummary) {
        if summary.index != self.height + 1 {
            // Ignore out-of-order commits. Higher layers enforce ordering.
            return;
        }
        self.height = summary.index;
        self.committed.push(summary);
        self.recalculate_totals();
    }

    pub fn rollback_to(&mut self, target_height: u64) {
        if target_height >= self.height {
            return;
        }
        while let Some(last) = self.committed.pop() {
            if last.index <= target_height {
                self.committed.push(last);
                break;
            }
            self.height -= 1;
        }
        self.height = self.committed.last().map(|b| b.index).unwrap_or(0);
        self.recalculate_totals();
    }

    pub fn sync_height(&mut self, target_height: u64) {
        if target_height == self.height {
            return;
        }
        self.committed.clear();
        if target_height > 0 {
            self.committed
                .push(BlockSummary::new(target_height, 0, 0));
        }
        self.height = target_height;
        self.recalculate_totals();
    }

    pub fn total_transactions(&self) -> u64 {
        self.total_transactions
    }

    pub fn total_fees(&self) -> u64 {
        self.total_fees
    }

    fn recalculate_totals(&mut self) {
        let mut txs = 0u64;
        let mut fees = 0u64;
        for summary in &self.committed {
            txs = txs.saturating_add(summary.transaction_count as u64);
            fees = fees.saturating_add(summary.fees_collected);
        }
        self.total_transactions = txs;
        self.total_fees = fees;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::txpool::PendingTransaction;

    #[test]
    fn queue_and_apply_blocks() {
        let mut chain = Blockchain::new();
        chain.queue_transaction(PendingTransaction::new("tx1", 100, 200));
        assert_eq!(chain.pending_transactions().count(), 1);

        let drained = chain.take_pending(10);
        assert_eq!(drained.len(), 1);

        chain.apply_block(BlockSummary::new(1, 1, 100));
        assert_eq!(chain.height(), 1);
        assert_eq!(chain.last_block().unwrap().transaction_count, 1);

        chain.apply_block(BlockSummary::new(3, 0, 0));
        // height stays unchanged because block index skipped.
        assert_eq!(chain.height(), 1);

        chain.apply_block(BlockSummary::new(2, 0, 0));
        assert_eq!(chain.height(), 2);
    }

    #[test]
    fn rollback_truncates_head() {
        let mut chain = Blockchain::new();
        chain.apply_block(BlockSummary::new(1, 1, 10));
        chain.apply_block(BlockSummary::new(2, 1, 10));
        chain.apply_block(BlockSummary::new(3, 1, 10));
        assert_eq!(chain.height(), 3);

        chain.rollback_to(1);
        assert_eq!(chain.height(), 1);
        assert_eq!(chain.last_block().unwrap().index, 1);
    }

    #[test]
    fn rollback_to_future_height_is_noop() {
        let mut chain = Blockchain::new();
        chain.apply_block(BlockSummary::new(1, 1, 10));
        chain.rollback_to(5);
        assert_eq!(chain.height(), 1);
    }
}

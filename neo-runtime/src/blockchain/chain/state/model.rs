use std::collections::VecDeque;

use crate::blockchain::BlockSummary;

#[derive(Debug, Default)]
pub struct Blockchain {
    pub(super) height: u64,
    pub(super) committed: VecDeque<BlockSummary>,
    pub(super) total_transactions: u64,
    pub(super) total_fees: u64,
    pub(super) total_bytes: u64,
}

impl Blockchain {
    pub fn new() -> Self {
        Self {
            height: 0,
            committed: VecDeque::new(),
            total_transactions: 0,
            total_fees: 0,
            total_bytes: 0,
        }
    }

    pub fn height(&self) -> u64 {
        self.height
    }

    pub fn last_block(&self) -> Option<&BlockSummary> {
        self.committed.back()
    }

    pub fn recent_blocks(&self) -> impl Iterator<Item = &BlockSummary> {
        self.committed.iter().rev()
    }

    pub fn total_transactions(&self) -> u64 {
        self.total_transactions
    }

    pub fn total_fees(&self) -> u64 {
        self.total_fees
    }

    pub fn total_size_bytes(&self) -> u64 {
        self.total_bytes
    }
}

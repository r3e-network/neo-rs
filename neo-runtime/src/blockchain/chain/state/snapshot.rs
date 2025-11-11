use alloc::vec::Vec;

use super::{Blockchain, RECENT_BLOCK_LIMIT};
use crate::blockchain::BlockchainSnapshot;

impl Blockchain {
    pub fn snapshot(&self) -> BlockchainSnapshot {
        BlockchainSnapshot {
            height: self.height,
            last_block: self.last_block().cloned(),
            total_transactions: self.total_transactions,
            total_fees: self.total_fees,
            total_bytes: self.total_bytes,
            recent_blocks: self.committed.iter().cloned().collect(),
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: BlockchainSnapshot) {
        self.height = snapshot.height;
        self.total_transactions = snapshot.total_transactions;
        self.total_fees = snapshot.total_fees;
        self.total_bytes = snapshot.total_bytes;
        self.committed.clear();
        let mut recent = snapshot.recent_blocks;
        if recent.is_empty() {
            if let Some(block) = snapshot.last_block {
                recent.push(block);
            }
        }
        if recent.len() > RECENT_BLOCK_LIMIT {
            recent = recent
                .into_iter()
                .rev()
                .take(RECENT_BLOCK_LIMIT)
                .collect::<Vec<_>>();
            recent.reverse();
        }
        for block in recent {
            self.committed.push_back(block);
        }
    }
}

//! Minimal runtime scaffolding for the Neo N3 Rust node. The runtime glues
//! together blockchain bookkeeping, transaction pooling, and fee estimation.

mod blockchain;
mod fee;
mod txpool;

pub use blockchain::{BlockSummary, Blockchain};
pub use fee::FeeCalculator;
pub use txpool::{PendingTransaction, TxPool};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeStats {
    pub height: u64,
    pub base_fee: u64,
    pub byte_fee: u64,
    pub mempool_size: usize,
    pub total_transactions: u64,
    pub total_fees: u64,
}

/// High-level runtime facade composing core subsystems.
#[derive(Debug)]
pub struct Runtime {
    blockchain: Blockchain,
    tx_pool: TxPool,
    fees: FeeCalculator,
}

impl Runtime {
    pub fn new(base_fee: u64, byte_fee: u64) -> Self {
        Self {
            blockchain: Blockchain::new(),
            tx_pool: TxPool::new(),
            fees: FeeCalculator::new(base_fee, byte_fee),
        }
    }

    pub fn blockchain(&self) -> &Blockchain {
        &self.blockchain
    }

    pub fn tx_pool(&self) -> &TxPool {
        &self.tx_pool
    }

    pub fn tx_pool_mut(&mut self) -> &mut TxPool {
        &mut self.tx_pool
    }

    pub fn pending_ids(&self) -> impl Iterator<Item = &String> {
        self.tx_pool.ids()
    }

    pub fn fee_calculator(&self) -> &FeeCalculator {
        &self.fees
    }

    pub fn fee_calculator_mut(&mut self) -> &mut FeeCalculator {
        &mut self.fees
    }

    pub fn queue_transaction(&mut self, tx: PendingTransaction) -> bool {
        let inserted = self.tx_pool.insert(tx);
        self.fees.adjust_for_load(self.tx_pool.len(), 1024);
        inserted
    }

    pub fn commit_block(&mut self, summary: BlockSummary) {
        self.blockchain.apply_block(summary);
        self.fees.adjust_for_load(self.tx_pool.len(), 1024);
    }

    pub fn sync_height(&mut self, height: u64) {
        self.blockchain.sync_height(height);
    }

    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            height: self.blockchain.height(),
            base_fee: self.fees.base_fee(),
            byte_fee: self.fees.byte_fee(),
            mempool_size: self.tx_pool.len(),
            total_transactions: self.blockchain.total_transactions(),
            total_fees: self.blockchain.total_fees(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_queues_and_commits() {
        let mut runtime = Runtime::new(100, 2);
        assert!(runtime.queue_transaction(PendingTransaction::new("tx1", 10, 120)));
        let reserved = runtime.tx_pool_mut().reserve_for_block(5, 10_000);
        assert_eq!(reserved.len(), 1);
        runtime.commit_block(BlockSummary::new(1, 1, 10));
        assert_eq!(runtime.blockchain().height(), 1);
        assert!(runtime.fee_calculator().estimate(10) >= 120);
    }

    #[test]
    fn runtime_syncs_height() {
        let mut runtime = Runtime::new(50, 1);
        runtime.sync_height(5);
        assert_eq!(runtime.blockchain().height(), 5);
    }
}

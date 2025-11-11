use crate::{
    blockchain::{BlockSummary, Blockchain},
    fee::FeeCalculator,
    txpool::{PendingTransaction, TxPool},
};

pub struct Runtime {
    pub(super) blockchain: Blockchain,
    pub(super) tx_pool: TxPool,
    pub(super) fees: FeeCalculator,
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

    pub fn find_pending(&self, id: &str) -> Option<PendingTransaction> {
        self.tx_pool.get(id).cloned()
    }

    pub fn commit_block(&mut self, summary: BlockSummary) {
        self.blockchain.apply_block(summary);
        self.fees.adjust_for_load(self.tx_pool.len(), 1024);
    }

    pub fn sync_height(&mut self, height: u64) {
        self.blockchain.sync_height(height);
    }
}

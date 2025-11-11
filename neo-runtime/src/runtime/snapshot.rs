use alloc::vec::Vec;

use crate::{blockchain::BlockchainSnapshot, txpool::PendingTransaction};

#[derive(Debug, Clone)]
pub struct RuntimeSnapshot {
    pub blockchain: BlockchainSnapshot,
    pub base_fee: u64,
    pub byte_fee: u64,
    pub pending: Vec<PendingTransaction>,
}
